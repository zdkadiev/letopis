#[cfg(test)]
mod input_tests {
    use crate::cli::input::{Request, request_parse};
    use crate::service::{CommandErrorKind, CommandResult, ElementAddress};
    use std::{
        ffi::OsString,
        io::{self, Read},
    };

    #[test]
    fn empty_input_uses_empty_request_and_checks_required_fields() {
        // Given: commands with optional and required request fields.
        for body in ["", " \n\t"] {
            // When / Then: absent input behaves as an empty JSON object.
            assert!(matches!(parse(&["config"], body), Ok(Request::Config)));
            assert!(matches!(parse(&["status"], body), Ok(Request::Status)));
            assert!(matches!(parse(&["check"], body), Ok(Request::Check(_))));
            for command in ["search", "inspect", "traverse"] {
                assert_eq!(
                    parse(&[command], body).unwrap_err().get_kind(),
                    CommandErrorKind::InvalidRequest
                );
            }
        }
    }

    #[test]
    fn parse_check_requires_object_and_rejects_unknown_or_duplicate_fields() {
        // Given
        for body in ["{}", r#"{"paths":[]}"#] {
            // When / Then
            assert!(
                matches!(parse(&["check"], body).unwrap(), Request::Check(paths) if paths.is_empty())
            );
        }
        // Given
        for body in [
            "null",
            "[]",
            r#"{"paths":[],"paths":[]}"#,
            r#"{"paths":null}"#,
            r#"{"paths":[1]}"#,
            r#"{"paths":false}"#,
            r#"{"paths":["../escape"]}"#,
            r#"{"extra":1}"#,
            "{} {}",
        ] {
            // When / Then
            assert_eq!(
                parse(&["check"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest,
                "{body}"
            );
        }
    }

    #[test]
    fn status_accepts_only_empty_object_and_no_extra_argv() {
        // Given / When / Then
        assert!(matches!(parse(&["status"], "{}"), Ok(Request::Status)));
        // Given
        for body in [
            "null",
            "[]",
            r#"{"extra":1}"#,
            r#"{"extra":null}"#,
            r#"{"extra":1,"extra":2}"#,
            "{} {}",
        ] {
            // When / Then
            assert_eq!(
                parse(&["status"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
        // Given / When / Then
        assert_eq!(
            parse(&["status", "extra"], "{}").unwrap_err().get_kind(),
            CommandErrorKind::InvalidRequest
        );
    }

    #[test]
    fn parse_inspect_normalizes_without_resolving_membership() {
        // Given / When
        let request = parse(&["inspect"], r#"{"element_ids":["./x.md","a/../x.md","./x.md#A","x.md#A","s.txt#L01-L02","s.txt#L1-L2"]}"#).unwrap();
        let Request::Inspect(ids) = request else {
            panic!("expected inspection");
        };
        // Then
        assert_eq!(
            ids.iter().map(ElementAddress::as_str).collect::<Vec<_>>(),
            ["s.txt#L1-L2", "x.md", "x.md#A"]
        );
        // Given
        for ids in [
            vec![],
            vec![""],
            vec!["../a.md"],
            vec!["/a.md"],
            vec!["a\u{0}b"],
            vec!["a\u{1f}b"],
            vec!["a#L3-L2"],
            vec!["a#L1-L18446744073709551616"],
        ] {
            let body = serde_json::json!({"element_ids":ids}).to_string();
            // When / Then
            assert_eq!(
                parse(&["inspect"], &body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn inspect_rejects_wrong_shapes_unknown_and_duplicate_fields() {
        // Given
        for body in [
            "",
            "null",
            "[]",
            "{}",
            r#"{"element_ids":null}"#,
            r#"{"element_ids":[1]}"#,
            r#"{"element_ids":true}"#,
            r#"{"element_ids":["x"],"extra":1}"#,
            r#"{"element_ids":["x"],"element_ids":["x"]}"#,
            r#"{"element_ids":["x"]} {}"#,
        ] {
            // When / Then
            assert_eq!(
                parse(&["inspect"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest,
                "{body}"
            );
        }
    }

    #[test]
    fn drain_accepts_only_eof_or_empty_object() {
        // Given
        for body in ["", " \n\t", "{}"] {
            // When / Then
            assert!(matches!(
                parse(&["embed", "--drain"], body).unwrap(),
                Request::Drain
            ));
        }
        // Given
        for body in ["null", "[]", r#"{"extra":1}"#, "{} {}", "\u{b}", "\u{c}"] {
            // When / Then
            assert_eq!(
                parse(&["embed", "--drain"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn parse_argv_rejects_non_utf8_flags_and_surplus() {
        // Given: unsupported argv forms and input that must remain unread.
        use std::os::unix::ffi::OsStringExt;
        struct Unread;

        impl Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("argv must be validated before input");
            }
        }
        let mut cases = vec![
            vec![],
            vec!["unknown".into()],
            vec!["embed".into()],
            vec!["embed".into(), "--drain".into(), "--drain".into()],
            vec!["inspect".into(), "--drain".into()],
            vec!["check".into(), "extra".into()],
        ];
        cases.push(vec![OsString::from_vec(vec![255])]);
        cases.push(vec!["embed".into(), OsString::from_vec(vec![255])]);
        for args in cases {
            // When / Then: each argv is rejected before reading input.
            assert_eq!(
                request_parse(&args, &mut Unread).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }
    #[test]
    fn input_io_failure_is_redacted_invalid_request() {
        // Given: an input reader whose I/O error contains a secret sentinel.
        struct BrokenInput;

        impl Read for BrokenInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("SECRET_SENTINEL"))
            }
        }
        for args in [
            vec!["check".into()],
            vec!["inspect".into()],
            vec!["embed".into(), "--drain".into()],
        ] {
            // When: each supported request attempts to read that input.
            let error = request_parse(&args, &mut BrokenInput).unwrap_err();
            // Then: invalid_request is returned without exposing the private cause.
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
            assert!(!error.to_string().contains("SECRET_SENTINEL"));
        }
    }

    fn parse(command: &[&str], body: &str) -> CommandResult<Request> {
        let args = command.iter().map(OsString::from).collect::<Vec<_>>();
        request_parse(&args, &mut body.as_bytes())
    }
}

#[cfg(test)]
mod transport_tests {
    use crate::cli::output::response_write;
    use crate::cli::response::{CommandResponse, ResponseResult, finish_foreground};
    use crate::domain::{LineRange, RepoPath, Sha256, TreeOid};
    use crate::run_cli;
    use crate::service::{
        CheckReport, CommandError, CommandErrorKind, CommandProblem, CommandResult,
        ElementLocation, InspectedElement, Inspection, MockBackgroundRunner, StatusReport,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::io::{self, Write};

    #[test]
    fn check_findings_remain_successful() {
        // Given: a check report with one finding and a quiet background runner.
        let result = ResponseResult::Check(build_report());
        let mut background = build_background(1, 0);
        // When: the foreground command finishes.
        let (exit, value) = render(finish_foreground(result, &mut background));
        // Then: the finding is part of a successful envelope.
        assert_eq!(exit, 0);
        assert_eq!(
            value,
            json!({"ok":true,"result":{"errors":[{"code":"validation_failed","message":"safe finding","details":{"path":"a.md"}}]}})
        );
    }
    #[test]
    fn status_renders_all_fields() {
        // Given: a status report with every field set.
        let report = StatusReport {
            revision: Some(7),
            embedding_backlog: 2,
            embedding_failed: 1,
            locks: Vec::new(),
            problems: vec![make_problem()],
        };
        // When: the response is rendered.
        let response = Ok(CommandResponse::new(ResponseResult::Status(report)));
        // Then: all fields are present and no background work is implied.
        assert_eq!(
            render(response),
            (
                0,
                json!({"ok":true,"result":{
                    "revision":7,"embedding_backlog":2,"embedding_failed":1,"locks":[],
                    "problems":[{"code":"validation_failed","message":"safe finding","details":{"path":"a.md"}}]
                }})
            )
        );
    }
    #[test]
    fn failure_renders_one_safe_error_without_cause() {
        // Given: an error whose cause holds a secret.
        let error = CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "safe status failure",
            io::Error::other("SECRET_STATUS"),
        );
        // When: the failure is rendered.
        let (exit, value) = render(Err(error));
        // Then: only the safe message is disclosed.
        assert_eq!(exit, 1);
        assert_eq!(
            value,
            json!({"ok":false,"error":{
                "code":"internal_error","message":"safe status failure","details":{}
            }})
        );
    }
    #[test]
    fn inspection_renders_only_metadata_and_optional_lines() {
        // Given: an inspection and a background runner that reports a launch warning.
        let result = ResponseResult::Inspect(build_inspection());
        let mut background = build_warning_background();
        // When: the foreground command finishes.
        let (exit, value) = render(finish_foreground(result, &mut background));
        // Then: only metadata is rendered, with the warning attached.
        assert_eq!(exit, 0);
        assert_eq!(value["result"]["snapshot"], "a".repeat(40));
        assert_eq!(
            value["result"]["elements"]["a.md"],
            json!({"path":"a.md","hash":Sha256::digest(b"private raw knowledge").to_hex()})
        );
        assert_eq!(
            value["result"]["elements"]["source#L1-L2"],
            json!({"path":"source","lines":[1,2],"hash":null})
        );
        assert_eq!(value["warnings"].as_array().unwrap().len(), 1);
        assert!(!value.to_string().contains("private raw knowledge"));
    }
    #[test]
    fn background_readiness_error_remains_failure() {
        // Given: a successful check result and a background runner that cannot get ready.
        let result = ResponseResult::Check(build_report());
        let mut background = build_failing_background();
        // When: the foreground command finishes.
        let error = finish_foreground(result, &mut background).unwrap_err();
        // Then: the readiness failure replaces the success.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    }
    #[test]
    fn render_every_catalogue_error_without_causes() {
        // Given
        for (kind, code) in [
            (CommandErrorKind::InvalidRequest, "invalid_request"),
            (CommandErrorKind::NotFound, "not_found"),
            (CommandErrorKind::ValidationFailed, "validation_failed"),
            (CommandErrorKind::EmbeddingFailed, "embedding_failed"),
            (CommandErrorKind::InternalError, "internal_error"),
            (CommandErrorKind::Cancelled, "cancelled"),
        ] {
            // Given
            let error = CommandError::new_with_cause(
                kind,
                "safe failure",
                io::Error::other("SECRET_SENTINEL"),
            );
            // When / Then
            assert_eq!(
                render(Err(error)),
                (
                    1,
                    json!({"ok":false,"error":{"code":code,"message":"safe failure","details":{}}})
                )
            );
        }
    }
    struct FailingWriter {
        remaining: usize,
        calls: usize,
        flushes: usize,
        bytes: Vec<u8>,
        is_flush_failing: bool,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.remaining == 0 {
                return Err(io::Error::other("output failed"));
            }
            let count = self.remaining.min(bytes.len());
            self.remaining -= count;
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.is_flush_failing {
                Err(io::Error::other("flush failed"))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn output_failures_do_not_attempt_a_second_envelope() {
        // Given: outputs that fail immediately or after a partial envelope.
        for (remaining, expected_calls) in [(0, 1), (5, 2)] {
            // Given: a writer with the selected byte limit and a safe command failure.
            let mut writer = FailingWriter {
                remaining,
                calls: 0,
                flushes: 0,
                bytes: vec![],
                is_flush_failing: false,
            };
            let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
            // When / Then: writing the failure response returns exit one.
            assert_eq!(response_write(&mut writer, Err(error)), 1);
            // Then: only the partial first envelope is attempted and no flush occurs.
            assert_eq!(writer.calls, expected_calls);
            assert_eq!(writer.bytes.len(), remaining);
            assert_eq!(writer.flushes, 0);
        }
    }
    #[test]
    fn flush_failure_returns_one_after_one_complete_envelope() {
        // Given: an output that accepts bytes but fails on its first flush.
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            is_flush_failing: true,
        };
        let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
        // When / Then: writing the failure response returns exit one.
        assert_eq!(response_write(&mut writer, Err(error)), 1);
        // Then: exactly one complete envelope was written and flushed once.
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes.last(), Some(&b'\n'));
        assert!(serde_json::from_slice::<Value>(&writer.bytes).is_ok());
    }
    #[test]
    fn success_writes_one_complete_envelope_and_flushes_once() {
        // Given: a successful drain response with an empty result.
        let response = Ok(CommandResponse::new(ResponseResult::Drained));
        // Given: an output that accepts every byte and its final flush.
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            is_flush_failing: false,
        };
        // When / Then: writing the response returns exit zero.
        assert_eq!(response_write(&mut writer, response), 0);
        // Then: one exact success envelope is written and flushed once.
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes, b"{\"ok\":true,\"result\":{}}\n");
    }
    #[test]
    fn check_without_background_warning_omits_warnings_field() {
        // Given
        let result = ResponseResult::Check(build_report());
        let mut background = build_background(1, 0);
        // When
        let (exit, value) = render(finish_foreground(result, &mut background));
        // Then
        assert_eq!(exit, 0);
        assert!(value.get("warnings").is_none());
    }
    #[test]
    fn run_cli_rejects_invalid_input() {
        // Given
        for (args, body, code) in [
            (vec!["check".into()], "null", "invalid_request"),
            (vec!["inspect".into()], "{}", "invalid_request"),
        ] {
            // Given
            let mut output = vec![];
            // When / Then
            assert_eq!(run_cli(args, &mut body.as_bytes(), &mut output), 1);
            // Then
            assert_eq!(
                serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
                code
            );
            assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        }
    }
    fn build_report() -> CheckReport {
        CheckReport {
            errors: vec![make_problem()],
        }
    }
    fn build_inspection() -> Inspection {
        Inspection {
            snapshot: TreeOid::parse(&"a".repeat(40)).unwrap(),
            elements: BTreeMap::from([
                (
                    "a.md".into(),
                    InspectedElement {
                        path: ElementLocation::Local(RepoPath::parse("a.md").unwrap()),
                        lines: None,
                        hash: Some(Sha256::digest(b"private raw knowledge")),
                    },
                ),
                (
                    "source#L1-L2".into(),
                    InspectedElement {
                        path: ElementLocation::Local(RepoPath::parse("source").unwrap()),
                        lines: Some(LineRange::new(1, 2).unwrap()),
                        hash: None,
                    },
                ),
            ]),
        }
    }
    fn build_background(starts: usize, drains: usize) -> MockBackgroundRunner {
        let mut background = MockBackgroundRunner::new();
        background
            .expect_start_if_needed()
            .times(starts)
            .returning(|| Ok(None));
        background.expect_drain().times(drains).returning(|| Ok(()));
        background
    }
    fn build_warning_background() -> MockBackgroundRunner {
        let mut background = MockBackgroundRunner::new();
        background
            .expect_start_if_needed()
            .times(1)
            .returning(|| Ok(Some(make_problem())));
        background.expect_drain().times(0);
        background
    }
    fn build_failing_background() -> MockBackgroundRunner {
        let mut background = MockBackgroundRunner::new();
        background.expect_start_if_needed().times(1).returning(|| {
            Err(CommandError::new(
                CommandErrorKind::InternalError,
                "safe background failure",
            ))
        });
        background.expect_drain().times(0);
        background
    }
    fn make_problem() -> CommandProblem {
        CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "safe finding",
            details: BTreeMap::from([("path".into(), json!("a.md"))]),
        }
    }
    fn render(response: CommandResult<CommandResponse>) -> (u8, Value) {
        let mut output = Vec::new();
        let exit = response_write(&mut output, response);
        assert_eq!(output.last(), Some(&b'\n'));
        let mut values = serde_json::Deserializer::from_slice(&output).into_iter::<Value>();
        let value = values.next().unwrap().unwrap();
        assert!(values.next().is_none());
        println!(
            "CLI_RESPONSE {}",
            serde_json::json!({"exit":exit,"response":value})
        );
        (exit, value)
    }
}

mod config_tests {
    use crate::cli::{CommandResponse, Request, ResponseResult, request_parse, response_write};
    use crate::service::{CommandErrorKind, Configuration};
    use serde_json::{Value, json};
    use std::{ffi::OsString, io::Read};

    const LITERAL_CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}}"#;

    #[test]
    fn rejects_nonempty_config_request_before_io() {
        // Given: invalid argv must fail before stdin or any service is read.
        let mut unread = UnreadInput;

        // When: the config argv includes an unsupported positional argument.
        let error = request_parse(
            [OsString::from("config"), OsString::from("extra")].as_slice(),
            &mut unread,
        )
        .unwrap_err();

        // Then: validation fails before the unreadable input is touched.
        assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);

        // Given: malformed or nonempty configuration request bodies.
        for body in [
            "null",
            "[]",
            r#"{"provider":"ollama"}"#,
            r#"{"extra":1}"#,
            r#"{"extra":1,"extra":2}"#,
            "{} {}",
        ] {
            // When: each body is parsed as a config request.
            let error =
                request_parse([OsString::from("config")].as_slice(), body.as_bytes()).unwrap_err();

            // Then: each unsupported body returns invalid_request.
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest, "{body}");
        }

        // Given / When / Then: an empty JSON object parses as the config request.
        assert!(matches!(
            request_parse([OsString::from("config")].as_slice(), b"{}".as_slice()).unwrap(),
            Request::Config
        ));
    }

    #[test]
    fn renders_exact_contract_object_without_warnings() {
        // Given: a configuration with literal public fields and no warnings.
        let config = Configuration::parse(LITERAL_CONFIG).unwrap();
        let response = Ok(CommandResponse::new(ResponseResult::Config(Some(config))));

        // When: the configuration response is written to a buffer.
        let mut output = Vec::new();
        let exit = response_write(&mut output, response);

        // Then: one successful JSON envelope contains exactly the public configuration.
        assert_eq!(exit, 0);
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            value,
            json!({"ok":true,"result":{"embedding_model":{
                "enabled":true,
                "provider":"ollama",
                "model":"fixture",
                "parameters":{
                    "base_url":"http://127.0.0.1:9",
                    "custom":{"enabled":true},
                    "dimensions":3
                },
                "max_retries":2
            },"discovery":{"respect_gitignore":true}}})
        );
        assert!(value.get("warnings").is_none());
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
    }

    struct UnreadInput;

    impl Read for UnreadInput {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("stdin must not be read before argv validation");
        }
    }
}

mod search_tests {
    //! Injected transport doubles only; real service behavior lives in binary tests.
    use crate::cli::{Request, ResponseResult, finish_foreground, request_parse, response_write};
    use crate::{
        domain::{KnowledgeId, LineRange, RepoPath},
        service::{
            CommandErrorKind, CommandResult, MockBackgroundRunner, RecheckFilter, SearchFlag,
            SearchLink,
        },
    };
    use serde_json::{Value, json};
    use std::{collections::BTreeSet, ffi::OsString};

    #[test]
    fn parse_search_rejects_null_duplicate_unknown_numeric_and_trailing_inputs() {
        // Given: search bodies with invalid shapes, fields, numbers or trailing input.
        for body in [
            "{}",
            r#"{"limit":1}"#,
            r#"{"query":null,"limit":1}"#,
            r#"{"query":1,"limit":1}"#,
            r#"{"query":"x","query":"y","limit":1}"#,
            r#"{"query":"x","limit":1,"limit":2}"#,
            r#"{"query":"x","limit":1,"filters":{},"filters":{}}"#,
            r#"{"query":"x","limit":null}"#,
            r#"{"query":"x","limit":0}"#,
            r#"{"query":"x","limit":-1}"#,
            r#"{"query":"x","limit":1.0}"#,
            r#"{"query":"x","limit":1e0}"#,
            r#"{"query":"x","limit":"1"}"#,
            r#"{"query":"x","limit":true}"#,
            r#"{"query":"x","limit":18446744073709551616}"#,
            r#"{"query":"x","limit":1,"extra":1}"#,
            r#"{"query":"x","limit":1,"filters":null}"#,
            r#"{"query":"x","limit":1,"filters":[]}"#,
            r#"{"query":"x","limit":1,"filters":{"needs_recheck":null}}"#,
            r#"{"query":"x","limit":1,"filters":{"needs_recheck":1}}"#,
            r#"{"query":"x","limit":1,"filters":{"needs_recheck":true,"needs_recheck":false}}"#,
            r#"{"query":"x","limit":1,"filters":{"sources":null}}"#,
            r#"{"query":"x","limit":1,"filters":{"sources":[null]}}"#,
            r#"{"query":"x","limit":1,"filters":{"sources":[],"sources":[]}}"#,
            r#"{"query":"x","limit":1,"filters":{"source_revisions":null}}"#,
            r#"{"query":"x","limit":1,"filters":{"source_revisions":[1]}}"#,
            r#"{"query":"x","limit":1,"filters":{"source_revisions":[],"source_revisions":[]}}"#,
            r#"{"query":"x","limit":1,"filters":{"unknown":1}}"#,
            r#"{"query":"x","limit":1,"filters":{"source_revisions":["bad"]}}"#,
            r#"{"query":"x","limit":1} {}"#,
            "null",
            "[]",
            r#"["x",1]"#,
            r#"["x",1,{}]"#,
            "",
        ] {
            // When: each body is parsed as a search request.
            let result = parse(body);
            // Then: each malformed body returns invalid_request.
            assert!(result.is_err(), "unexpected success: {body}");
            assert_eq!(
                result.unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest,
                "{body}"
            );
        }
        // Given / When / Then: a search argv with an extra argument is rejected.
        assert!(request_parse(&["search".into(), "extra".into()], &mut b"{}".as_slice()).is_err());
    }

    #[test]
    fn malformed_source_revisions_are_rejected() {
        // Given
        for oid in ["A".repeat(40), "a".repeat(39), "g".repeat(64)] {
            // Given
            let body =
                json!({"query":"x","limit":1,"filters":{"sources":["x"],"source_revisions":[oid]}});
            // When / Then
            assert_eq!(
                parse(&body.to_string()).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn empty_filters_equal_absence() {
        // Given
        for filters in [
            None,
            Some(json!({})),
            Some(json!({"sources":[],"source_revisions":[]})),
        ] {
            // Given
            let mut body = json!({"query":"private query","limit":3});
            if let Some(filters) = filters {
                body["filters"] = filters;
            }
            // When
            let Request::Search(request) = parse(&body.to_string()).unwrap() else {
                panic!("search")
            };
            // Then
            assert_eq!(request.query, "private query");
            assert_eq!(request.limit.get(), 3);
        }
    }

    #[test]
    fn effective_filters_are_typed_and_deduplicated() {
        // Given
        for (filters, recheck, sources, revisions) in [
            (json!({"needs_recheck":true}), RecheckFilter::Required, 0, 0),
            (
                json!({"needs_recheck":false}),
                RecheckFilter::NotRequired,
                0,
                0,
            ),
            (
                json!({"sources":["./src/../code.rs","code.rs"]}),
                RecheckFilter::Any,
                1,
                0,
            ),
            (
                json!({"source_revisions":["a".repeat(40),"a".repeat(40)]}),
                RecheckFilter::Any,
                0,
                1,
            ),
            (
                json!({"source_revisions":["a".repeat(64)]}),
                RecheckFilter::Any,
                0,
                1,
            ),
        ] {
            // When
            let Request::Search(request) =
                parse(&json!({"query":"x","limit":1,"filters":filters}).to_string()).unwrap()
            else {
                panic!("search")
            };
            // Then
            assert_eq!(request.filters.needs_recheck, recheck);
            assert_eq!(request.filters.sources.len(), sources);
            assert_eq!(request.filters.source_revisions.len(), revisions);
        }
    }

    #[test]
    fn search_response_renders_links() {
        // Given: one search link and a quiet background runner.
        let links = vec![SearchLink {
            element_id: KnowledgeId::parse("x.md#a").unwrap(),
            path: RepoPath::parse("x.md").unwrap(),
            lines: LineRange::new(2, 7).unwrap(),
            revision: 9,
            flags: BTreeSet::from([SearchFlag::Stale]),
        }];
        let mut background = MockBackgroundRunner::new();
        background
            .expect_start_if_needed()
            .times(1)
            .returning(|| Ok(None));
        // When: the foreground command finishes.
        let response = finish_foreground(ResponseResult::Search(links), &mut background);
        let mut output = Vec::new();
        let exit = response_write(&mut output, response);
        let value: Value = serde_json::from_slice(&output).unwrap();
        // Then: one success envelope preserves the link metadata and freshness flag.
        assert_eq!(exit, 0);
        assert_eq!(
            value,
            json!({"ok":true,"result":[{"element_id":"x.md#a","path":"x.md","lines":[2,7],"revision":9,"flags":["stale"]}]})
        );
        println!("CLI_RESPONSE {}", json!({"exit":exit,"response":value}));
    }

    #[test]
    fn search_defaults_and_caps_positive_limits() {
        // Given: omitted, small, boundary and oversized valid limits.
        for (input, expected) in [
            (r#"{"query":"sqlite"}"#, 100),
            (r#"{"query":"sqlite","limit":1}"#, 1),
            (r#"{"query":"sqlite","limit":5}"#, 5),
            (r#"{"query":"sqlite","limit":100}"#, 100),
            (r#"{"query":"sqlite","limit":101}"#, 100),
            (r#"{"query":"sqlite","limit":18446744073709551615}"#, 100),
        ] {
            // When: the search request is decoded.
            let Ok(Request::Search(request)) = parse(input) else {
                panic!("search request")
            };
            // Then: each request has its independently specified effective bound.
            assert_eq!(request.limit.get(), expected, "{input}");
        }
    }

    #[test]
    fn search_rejects_invalid_limits_without_defaulting_them() {
        // Given: supplied limits that are not positive unsigned integers.
        for limit in ["0", "null", "-1", "1.0", "1e0", "18446744073709551616"] {
            let input = format!(r#"{{"query":"sqlite","limit":{limit}}}"#);
            // When: the search request is decoded.
            let error = parse(&input).unwrap_err();
            // Then: an explicit invalid value is rejected rather than replaced by a default.
            assert_eq!(
                error.get_kind(),
                CommandErrorKind::InvalidRequest,
                "{limit}"
            );
        }
    }

    fn parse(body: &str) -> CommandResult<Request> {
        request_parse(&[OsString::from("search")], &mut body.as_bytes())
    }
}

mod traversal_tests {
    //! Strict traversal transport; graph semantics are exercised through the real binary.
    use crate::cli::{Request, ResponseResult, finish_foreground, request_parse, response_write};
    use crate::domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri};
    use crate::service::{
        CommandErrorKind, CommandProblem, CommandResult, ElementAddress, ElementLocation,
        MockBackgroundRunner, Traversal, TraversalDirection, TraversalLink, TraversalMode,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    #[test]
    fn accepts_required_traversal_fields_and_maximum_depth() {
        // Given
        for mode in ["relations", "containment", "both"] {
            for direction in ["outgoing", "incoming", "both"] {
                // Given
                let body = json!({"element_ids":["a.md"],"mode":mode,"direction":direction,
                    "depth":u64::MAX,"limit":1});
                // When / Then
                assert!(parse(&body.to_string()).is_ok(), "{body}");
            }
        }
    }

    #[test]
    fn accepts_representable_positive_caps_without_preallocating() {
        // Given
        let mut body = build_input();
        body["limit"] = json!(usize::MAX);
        body["edge_limit"] = json!(usize::MAX);
        // When
        let Request::Traverse(request) = parse(&body.to_string()).unwrap() else {
            panic!("traverse")
        };
        // Then
        assert_eq!(request.limit.get(), usize::MAX);
        assert_eq!(request.edge_limit.unwrap().get(), usize::MAX);
    }

    #[test]
    fn rejects_missing_required_traversal_fields() {
        // Given
        let body = build_input();
        for key in ["element_ids", "mode", "direction", "depth", "limit"] {
            // Given
            let mut candidate = body.clone();
            candidate.as_object_mut().unwrap().remove(key);
            // When / Then
            assert_invalid(&candidate.to_string());
        }
    }

    #[test]
    fn rejects_wrong_shapes_values_and_null_optionals() {
        // Given
        for (key, values) in [
            ("element_ids", r#"[null,[],[1],[""],["../escape"],"a.md"]"#),
            ("mode", r#"[null,"invalid",1,[],{"both":null}]"#),
            ("direction", r#"[null,"invalid",1,{}, {"both":null}]"#),
            ("depth", r#"[null,-1,true,"1"]"#),
            ("limit", r#"[null,0,-1,true,"1"]"#),
            ("edge_limit", r#"[null,0,-1,[],"1"]"#),
            ("edge_types", r#"[null,1,[1],[""],["bad type"]]"#),
        ] {
            let values: Vec<Value> = serde_json::from_str(values).unwrap();
            for value in values {
                // Given
                let mut body = build_input();
                body[key] = value;
                // When / Then
                assert_invalid(&body.to_string());
            }
        }
    }

    #[test]
    fn rejects_numeric_spellings_duplicate_unknown_and_trailing_fields() {
        // Given
        let base = r#""element_ids":["a.md"],"mode":"both","direction":"both","depth":0,"limit":1"#;
        for key in ["depth", "limit", "edge_limit"] {
            for number in ["1.0", "1e0", "-1", "18446744073709551616"] {
                // Given
                let mut body = build_input();
                body.as_object_mut().unwrap().remove(key);
                let prefix = body.to_string();
                let candidate = format!("{},\"{key}\":{number}}}", &prefix[..prefix.len() - 1]);
                // When / Then
                assert_invalid(&candidate);
            }
        }
        // Given
        for (key, value) in [
            ("element_ids", r#"["a.md"]"#),
            ("mode", r#""both""#),
            ("direction", r#""both""#),
            ("depth", "0"),
            ("limit", "1"),
        ] {
            // When / Then
            assert_invalid(&format!("{{{base},\"{key}\":{value}}}"));
        }
        // Given
        for suffix in [
            r#", "edge_types":[],"edge_types":[]"#,
            r#", "edge_limit":1,"edge_limit":2"#,
            r#", "unknown":true"#,
        ] {
            // When / Then
            assert_invalid(&format!("{{{base}{suffix}}}"));
        }
        // Given
        for body in ["", "null", "[]", "{} {}"] {
            // When / Then
            assert_invalid(body);
        }
        assert_invalid(&format!("{{{base}}} {{}}"));
    }

    #[test]
    fn normalizes_starts_and_open_types() {
        // Given: an input with relative, duplicate and case-varied addresses and edge types.
        let mut body = build_input();
        body["element_ids"] = json!([
            "./a.md",
            "sub/../a.md",
            "a.md",
            "src.rs#L01-L02",
            "src.rs#L1-L2",
            "https://Host/a?q=1#part",
            "http:notes.md"
        ]);
        body["edge_types"] = json!(["custom_relation9", "includes", "custom_relation9"]);
        body["edge_limit"] = json!(2);
        // When: the request is parsed.
        let Request::Traverse(request) = parse(&body.to_string()).unwrap() else {
            panic!("traverse")
        };
        // Then: addresses and edge types are normalized and deduplicated, and defaults are kept.
        assert_eq!(
            request
                .element_ids
                .iter()
                .map(ElementAddress::as_str)
                .collect::<Vec<_>>(),
            [
                "a.md",
                "http:notes.md",
                "https://Host/a?q=1#part",
                "src.rs#L1-L2"
            ]
        );
        assert_eq!(
            request
                .edge_types
                .iter()
                .map(EdgeType::as_str)
                .collect::<Vec<_>>(),
            ["custom_relation9", "includes"]
        );
        assert_eq!(request.edge_limit.unwrap().get(), 2);
        assert_eq!(
            (
                request.mode,
                request.direction,
                request.depth,
                request.limit.get()
            ),
            (TraversalMode::Both, TraversalDirection::Outgoing, 1, 3)
        );
    }

    #[test]
    fn absent_or_empty_edge_types_parse_without_a_limit() {
        // Given: an input without an edge type selection or with an empty one.
        for types in [None, Some(json!([]))] {
            let mut body = build_input();
            if let Some(types) = types {
                body["edge_types"] = types;
            }
            // When: the request is parsed.
            let Request::Traverse(request) = parse(&body.to_string()).unwrap() else {
                panic!("traverse")
            };
            // Then: no edge types and no edge limit remain on the request.
            assert!(request.edge_types.is_empty());
            assert!(request.edge_limit.is_none());
        }
    }

    #[test]
    fn rejects_malformed_identities_without_source_fallback() {
        // Given
        for id in [
            "http:notes",
            "https:/Host/a",
            "http://",
            "https://host/a b",
            "a#L3-L2",
            "a\u{0}b",
        ] {
            // Given
            let mut body = build_input();
            body["element_ids"] = json!([id]);
            // When / Then
            assert_invalid(&body.to_string());
        }
    }

    #[test]
    fn serializes_remote_source_links_without_lines() {
        // Given
        let response = Ok(crate::cli::response::CommandResponse {
            result: crate::cli::response::ResponseResult::Traverse(Traversal {
                elements: vec![TraversalLink {
                    element_id: ElementAddress::parse("https://Host/a?q=1#part").unwrap(),
                    path: ElementLocation::Remote(
                        SourceUri::parse("https://Host/a?q=1#part").unwrap(),
                    ),
                    lines: None,
                    revision: 7,
                    flags: BTreeSet::new(),
                }],
                edges: None,
            }),
            warnings: vec![],
        });
        // When
        let (exit, json) = render(response);
        // Then
        assert_eq!(exit, 0);
        assert_eq!(
            json,
            serde_json::json!({"ok":true,"result":{"elements":[{
                "element_id":"https://Host/a?q=1#part", "path":"https://Host/a?q=1#part",
                "revision":7,"flags":[]
            }]}})
        );
    }

    #[test]
    fn serializes_requested_empty_and_original_incoming_edges() {
        // Given
        for (edges, expected) in [
            (
                Some(vec![]),
                json!({"ok":true,"result":{"elements":[],"edges":[]}}),
            ),
            (
                Some(vec![
                    Edge::new(
                        KnowledgeId::parse("a.md").unwrap(),
                        EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
                        EdgeType::parse("supports").unwrap(),
                    )
                    .unwrap(),
                ]),
                json!({"ok":true,"result":{"elements":[],"edges":[{"source":"a.md","target":"b.md","type":"supports"}]}}),
            ),
        ] {
            // Given
            let response = Ok(crate::cli::response::CommandResponse {
                result: crate::cli::response::ResponseResult::Traverse(Traversal {
                    elements: vec![],
                    edges,
                }),
                warnings: vec![],
            });
            // When / Then
            assert_eq!(render(response), (0, expected));
        }
    }

    #[test]
    fn traversal_response_preserves_nonfatal_warning() {
        // Given: an empty traversal and a background runner that cannot launch the drain.
        let mut background = MockBackgroundRunner::new();
        background.expect_start_if_needed().times(1).returning(|| {
            Ok(Some(CommandProblem {
                code: CommandErrorKind::InternalError,
                message: "cannot start background embedding drain",
                details: Default::default(),
            }))
        });
        background.expect_drain().times(0);
        let result = ResponseResult::Traverse(Traversal {
            elements: vec![],
            edges: None,
        });
        // When: the foreground command finishes.
        let response = finish_foreground(result, &mut background);
        // Then: the result is still successful and carries the warning.
        assert_eq!(
            render(response),
            (
                0,
                json!({"ok":true,"result":{"elements":[]},
            "warnings":[{"code":"internal_error","message":"cannot start background embedding drain","details":{}}]})
            )
        );
    }

    fn render(response: CommandResult<crate::cli::response::CommandResponse>) -> (u8, Value) {
        let mut bytes = vec![];
        let exit = response_write(&mut bytes, response);
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        (exit, serde_json::from_slice(&bytes).unwrap())
    }

    fn build_input() -> Value {
        json!({"element_ids":["a.md"],"mode":"both","direction":"outgoing","depth":1,"limit":3})
    }

    fn assert_invalid(body: &str) {
        assert_eq!(
            parse(body).unwrap_err().get_kind(),
            CommandErrorKind::InvalidRequest,
            "{body}"
        );
    }

    fn parse(body: &str) -> CommandResult<Request> {
        request_parse(&["traverse".into()], body.as_bytes())
    }
}

mod help_tests {
    use crate::cli::help::{COMMANDS, help_get_by_args};
    use crate::cli::input::{Request, request_parse};
    use crate::cli::output::response_write;
    use crate::cli::response::{CommandResponse, ResponseResult};
    use crate::run_cli;
    use crate::service::CommandResult;
    use serde_json::Value;
    use std::{
        ffi::OsString,
        io::{self, Read, Write},
        os::unix::ffi::OsStringExt,
    };

    const COMMAND_NAMES: [&str; 8] = [
        "init", "check", "inspect", "search", "traverse", "index", "status", "config",
    ];

    #[test]
    fn config_is_discoverable_and_describes_read_only_configuration() {
        // Given: the general and config-specific help requests.
        let general_args = to_args(&["--help"]);
        let config_args = to_args(&["config", "--help"]);
        // When: help is requested without a repository or input.
        let general = help_get_by_args(&general_args).unwrap();
        let config = help_get_by_args(&config_args);
        // Then: both describe the command and its configuration file.
        assert!(
            general
                .lines()
                .any(|line| line.trim_start().starts_with("config "))
        );
        let config = config.unwrap();
        assert!(config.contains(".letopis/config.json"));
        assert!(config.contains("null"));
        assert!(config.contains("Edit"));
    }

    #[test]
    fn find_help_recognizes_flags_and_every_command() {
        // Given: the supported CLI flags and known command names.
        // When / Then: version and general help return their expected text.
        assert_eq!(
            help_get_by_args(&to_args(&["--version"])).as_deref(),
            Some("letopis 0.2.0\n")
        );
        assert!(
            help_get_by_args(&to_args(&["--help"]))
                .unwrap()
                .starts_with("Usage")
        );
        for name in COMMAND_NAMES {
            // When: help is requested for a known command.
            let text = help_get_by_args(&to_args(&[name, "--help"])).unwrap();
            // Then: the output identifies that command.
            assert!(text.contains(name), "{name}");
        }
    }

    #[test]
    fn find_help_ignores_everything_else() {
        // Given: argv shapes outside the supported help and version forms.
        let cases: [&[&str]; 10] = [
            &[],
            &["-h"],
            &["-V"],
            &["--help", "search"],
            &["search", "--help", "x"],
            &["--version", "x"],
            &["embed", "--help"],
            &["embed", "--drain"],
            &["--bogus"],
            &["status"],
        ];
        for args in cases {
            // When / Then: each unsupported form returns no help text.
            assert_eq!(help_get_by_args(&to_args(args)), None, "{args:?}");
        }
    }

    #[test]
    fn find_help_ignores_non_utf8_argv() {
        // Given: an argument containing invalid UTF-8 bytes.
        let bad = OsString::from_vec(vec![0xff]);
        // When / Then: neither that argument nor its help-suffixed form matches help.
        assert_eq!(help_get_by_args(std::slice::from_ref(&bad)), None);
        assert_eq!(help_get_by_args(&[bad, "--help".into()]), None);
    }

    #[test]
    fn run_cli_prints_help_without_reading_stdin() {
        // Given: output buffers and a reader that panics if stdin is read.
        let mut help = vec![];
        let mut version = vec![];
        // When: help and version are requested.
        let help_exit = run_cli(to_args(&["--help"]), PanicReader, &mut help);
        let version_exit = run_cli(to_args(&["--version"]), PanicReader, &mut version);
        // Then: both succeed as plain text without reading stdin.
        assert_eq!(help_exit, 0);
        assert!(String::from_utf8(help).unwrap().starts_with("Usage"));
        assert_eq!(version_exit, 0);
        assert_eq!(version, b"letopis 0.2.0\n");
    }

    #[test]
    fn run_cli_keeps_json_error_for_unrecognized_help_forms() {
        // Given
        for args in [&["--help", "search"][..], &[][..]] {
            let mut output = vec![];
            // When
            let exit = run_cli(to_args(args), &mut "".as_bytes(), &mut output);
            // Then
            assert_eq!(exit, 1, "{args:?}");
            let value = serde_json::from_slice::<Value>(&output).unwrap();
            assert_eq!(value["error"]["code"], "invalid_request");
        }
    }

    #[test]
    fn parse_request_returns_help_without_reading_stdin() {
        // Given: stdin that panics if it is read.
        let args = to_args(&["search", "--help"]);
        // When: the help form is parsed.
        let request = request_parse(&args, PanicReader);
        // Then: it is an ordinary typed request carrying the help text.
        assert!(matches!(request, Ok(Request::Help(text)) if text.contains("letopis search")));
    }

    #[test]
    fn write_response_prints_help_as_plain_text_and_reports_write_failure() {
        // Given: a help response for a text with no JSON envelope.
        let help = || -> CommandResult<CommandResponse> {
            Ok(CommandResponse::new(ResponseResult::Help("x".into())))
        };
        let mut written = vec![];
        // When / Then: it is written verbatim with exit 0.
        assert_eq!(response_write(&mut written, help()), 0);
        assert_eq!(written, b"x");
        // When / Then: a failing writer maps to exit 1.
        assert_eq!(response_write(FailingWriter, help()), 1);
    }

    #[test]
    fn command_names_match_literal_list() {
        // Given / When
        let names = COMMANDS.map(|command| command.name);
        // Then
        assert_eq!(names, COMMAND_NAMES);
    }

    #[test]
    fn every_command_example_is_a_valid_request() {
        for command in COMMANDS {
            // Given: the published example for each command.
            let args = [OsString::from(command.name)];
            // When: the example is parsed using that command name.
            let request = request_parse(&args, &mut command.example.as_bytes());
            // Then: the copyable example is accepted as a valid request.
            assert!(request.is_ok(), "{}: {}", command.name, command.example);
        }
    }

    #[test]
    fn every_command_help_shows_its_example_and_usage() {
        for command in COMMANDS {
            // Given / When: each supported command is requested with --help.
            let text = help_get_by_args(&to_args(&[command.name, "--help"])).unwrap();
            // Then: the response contains that command usage and its literal example.
            assert!(text.contains(command.example), "{}", command.name);
            assert!(
                text.contains(&format!("letopis {}", command.name)),
                "{}",
                command.name
            );
        }
    }

    #[test]
    fn general_help_covers_contract() {
        // Given / When: the exact --help form requests general help.
        let text = help_get_by_args(&to_args(&["--help"])).unwrap();
        // Then: the text describes public commands and transport without exposing drain.
        for needle in COMMAND_NAMES.into_iter().chain([
            "invalid_request",
            "not_found",
            "validation_failed",
            "embedding_failed",
            "internal_error",
            "cancelled",
            "stdin",
            "\"ok\":true",
            "warnings",
        ]) {
            assert!(text.contains(needle), "{needle}");
        }
        assert!(!text.contains("--drain"));
    }

    #[test]
    fn help_lines_fit_eighty_columns_except_copyable_examples() {
        for command in COMMANDS {
            // Given: general help and each command-specific help text.
            let general = help_get_by_args(&to_args(&["--help"])).unwrap();
            let specific = help_get_by_args(&to_args(&[command.name, "--help"])).unwrap();
            // When / Then: every non-example line fits within eighty columns.
            // Example lines stay on one line so they can be pasted into a shell.
            for line in general.lines().chain(specific.lines()) {
                let is_example = line.contains("printf ");
                assert!(is_example || line.chars().count() <= 80, "{line}");
            }
        }
    }

    #[test]
    fn command_help_states_side_effects_and_value_forms() {
        // Given: a help lookup for each public foreground command.
        let help = |name: &str| help_get_by_args(&to_args(&[name, "--help"])).unwrap();
        // When: check, inspect, search and traverse help is rendered.
        let (check, inspect, search, traverse) = (
            help("check"),
            help("inspect"),
            help("search"),
            help("traverse"),
        );
        // Then: each text states its relevant side effects or accepted value forms.
        assert!(!check.contains("Never writes files"));
        assert!(check.contains("refreshes the index cache"));
        assert!(search.contains("http(s) URL"));
        assert!(inspect.contains("sha256 hex or null"));
        assert!(traverse.contains("[a-z][a-z0-9_]*"));
        assert!(traverse.contains("includes") && traverse.contains("external"));
    }

    fn to_args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    struct PanicReader;

    impl Read for PanicReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("help must not read stdin");
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

mod index_tests {
    use crate::cli::{IndexMode, Request, request_parse};
    use crate::service::CommandErrorKind;
    use std::{
        ffi::OsString,
        io::{self, Read},
    };

    #[test]
    fn accepts_index_modes_and_deduplicates_scoped_paths() {
        // Given: every valid argv form and a duplicate path selector.
        let cases: &[(&[&str], IndexMode)] = &[
            (&["index"], IndexMode::Incremental),
            (&["index", "--verify"], IndexMode::Verified),
            (&["index", "--full"], IndexMode::Full),
            (&["index", "--full", "--verify"], IndexMode::Full),
            (&["index", "--verify", "--full"], IndexMode::Full),
        ];
        for (args, expected) in cases {
            // When: the command parses its body.
            let args: Vec<OsString> = args.iter().map(OsString::from).collect();
            let result =
                request_parse(&args, br#"{"paths":["notes/a","notes/a"]}"#.as_slice()).unwrap();
            // Then: the mode comes from argv and paths are normalized once.
            let Request::Index(request) = result else {
                panic!("expected index request")
            };
            assert_eq!(request.mode, *expected);
            assert_eq!(request.paths.len(), 1);
            assert_eq!(request.paths[0].as_str(), "notes/a");
        }
    }

    #[test]
    fn rejects_unsupported_index_argv_before_reading_stdin() {
        // Given: a reader that fails if any bytes are requested.
        for args in [
            vec!["index", "--verify", "--verify"],
            vec!["index", "--full", "--full"],
            vec!["index", "--unknown"],
            vec!["index", "extra"],
        ] {
            // When: argv validation precedes stdin.
            let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
            let error = request_parse(&args, PanicReader).unwrap_err();
            // Then: only the safe invalid request classification is returned.
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
        }
    }

    #[test]
    fn rejects_invalid_index_json_without_exposing_input() {
        // Given: malformed objects and unsafe path data.
        for body in [
            "null",
            "[]",
            r#"{"paths":null}"#,
            r#"{"paths":["../escape"]}"#,
            r#"{"paths":[],"paths":[]}"#,
            r#"{"command":"index"}"#,
        ] {
            // When: index validates the JSON object.
            let error = request_parse(&["index".into()], body.as_bytes()).unwrap_err();
            // Then: no malformed input becomes a usable scope.
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
        }
    }

    struct PanicReader;

    impl Read for PanicReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("stdin read before argv validation")
        }
    }
}
