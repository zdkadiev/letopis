#[cfg(test)]
mod input_tests {
    use crate::cli::input::{Request, parse_request};
    use crate::service::{CommandErrorKind, CommandResult, ElementAddress};
    use std::{
        ffi::OsString,
        io::{self, Read},
    };

    fn parse(command: &[&str], body: &str) -> CommandResult<Request> {
        let args = command.iter().map(OsString::from).collect::<Vec<_>>();
        parse_request(&args, &mut body.as_bytes())
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
            "",
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
            "",
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
        // Given
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
            // When / Then
            assert_eq!(
                parse_request(&args, &mut Unread).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }
    #[test]
    fn input_io_failure_is_redacted_invalid_request() {
        // Given
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
            // When
            let error = parse_request(&args, &mut BrokenInput).unwrap_err();
            // Then
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
            assert!(!error.to_string().contains("SECRET_SENTINEL"));
        }
    }
}

#[cfg(test)]
mod transport_tests {
    use crate::cli::commands::{
        CommandResponse, execute_check, execute_drain, execute_inspect, execute_status,
    };
    use crate::cli::input::{Request, parse_request};
    use crate::cli::output::write_response;
    use crate::domain::{LineRange, RepoPath, Sha256, TreeOid};
    use crate::run_cli;
    use crate::service::{
        CheckReport, CommandError, CommandErrorKind, CommandProblem, CommandResult, ElementAddress,
        ElementLocation, InspectedElement, Inspection, MockBackgroundRunner, MockChecker,
        MockInspector, MockStatusReader, StatusReport,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::io::{self, Write};

    #[test]
    fn check_dispatches_once_and_findings_remain_successful() {
        // Given
        let args = ["check".into()];
        let Request::Check(paths) = parse_request(
            &args,
            &mut br#"{"paths":["./a.md","b/../a.md","B.md"]}"#.as_slice(),
        )
        .unwrap() else {
            panic!("expected a check request");
        };
        let mut checks = build_checks(vec![
            RepoPath::parse("B.md").unwrap(),
            RepoPath::parse("a.md").unwrap(),
        ]);
        let mut background = build_background(1, 0);
        // When
        let response = execute_check(&mut checks, &paths, &mut background);
        let (exit, value) = render(response);
        // Then
        assert_eq!(exit, 0);
        assert_eq!(
            value,
            json!({"ok":true,"result":{"errors":[{"code":"validation_failed","message":"safe finding","details":{"path":"a.md"}}]}})
        );
    }
    #[test]
    fn status_renders_all_fields_without_background_work() {
        // Given
        let mut status = MockStatusReader::new();
        status.expect_read_status().times(1).returning(|| {
            Ok(StatusReport {
                revision: Some(7),
                embedding_backlog: 2,
                embedding_failed: 1,
                locks: Vec::new(),
                problems: vec![make_problem()],
            })
        });

        // When
        let response = execute_status(&mut status);

        // Then
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
    fn status_failure_renders_one_safe_error_without_background_work() {
        // Given
        let mut status = MockStatusReader::new();
        status.expect_read_status().times(1).returning(|| {
            Err(CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "safe status failure",
                io::Error::other("SECRET_STATUS"),
            ))
        });

        // When
        let response = execute_status(&mut status);

        // Then
        assert_eq!(
            render(response),
            (
                1,
                json!({"ok":false,"error":{
                    "code":"internal_error","message":"safe status failure","details":{}
                }})
            )
        );
    }
    #[test]
    fn inspection_renders_only_metadata_and_optional_lines() {
        // Given
        let mut inspections = build_inspection();
        let ids = vec![ElementAddress::parse("x.md").unwrap()];
        let mut background = build_warning_background();
        // When
        let response = execute_inspect(&mut inspections, &ids, &mut background);
        let (exit, value) = render(response);
        // Then
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
    fn drain_delegates_once_without_recursion() {
        // Given
        let mut background = build_background(0, 1);
        // When
        let response = execute_drain(&mut background);
        // Then
        assert_eq!(render(response), (0, json!({"ok":true,"result":{}})));
    }
    #[test]
    fn failed_service_does_not_start_background_or_disclose_cause() {
        // Given
        let mut checks = MockChecker::new();
        checks
            .expect_check()
            .times(1)
            .withf(<[RepoPath]>::is_empty)
            .returning(|_| {
                Err(CommandError::new_with_cause(
                    CommandErrorKind::ValidationFailed,
                    "safe failure",
                    io::Error::other("SECRET_SENTINEL"),
                ))
            });
        let mut background = build_background(0, 0);
        // When
        let response = execute_check(&mut checks, &[], &mut background);
        // Then
        assert_eq!(
            render(response),
            (
                1,
                json!({"ok":false,"error":{"code":"validation_failed","message":"safe failure","details":{}}})
            )
        );
    }
    #[test]
    fn background_readiness_error_remains_failure() {
        // Given
        let mut checks = build_checks(vec![]);
        let mut background = build_failing_background();
        // When
        let error = execute_check(&mut checks, &[], &mut background).unwrap_err();
        // Then
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
        fail_flush: bool,
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
            if self.fail_flush {
                Err(io::Error::other("flush failed"))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn output_failures_do_not_attempt_a_second_envelope() {
        // Given
        for (remaining, expected_calls) in [(0, 1), (5, 2)] {
            // Given
            let mut writer = FailingWriter {
                remaining,
                calls: 0,
                flushes: 0,
                bytes: vec![],
                fail_flush: false,
            };
            let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
            // When / Then
            assert_eq!(write_response(&mut writer, Err(error)), 1);
            // Then
            assert_eq!(writer.calls, expected_calls);
            assert_eq!(writer.bytes.len(), remaining);
            assert_eq!(writer.flushes, 0);
        }
    }
    #[test]
    fn flush_failure_returns_one_after_one_complete_envelope() {
        // Given
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            fail_flush: true,
        };
        let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
        // When / Then
        assert_eq!(write_response(&mut writer, Err(error)), 1);
        // Then
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes.last(), Some(&b'\n'));
        assert!(serde_json::from_slice::<Value>(&writer.bytes).is_ok());
    }
    #[test]
    fn success_writes_one_complete_envelope_and_flushes_once() {
        // Given
        let mut background = build_background(0, 1);
        // When
        let response = execute_drain(&mut background);
        // Given
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            fail_flush: false,
        };
        // When / Then
        assert_eq!(write_response(&mut writer, response), 0);
        // Then
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes, b"{\"ok\":true,\"result\":{}}\n");
    }
    #[test]
    fn check_without_background_warning_omits_warnings_field() {
        // Given
        let mut checks = build_checks(vec![]);
        let mut background = build_background(1, 0);
        // When
        let (exit, value) = render(execute_check(&mut checks, &[], &mut background));
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
    fn build_checks(paths: Vec<RepoPath>) -> MockChecker {
        let mut checks = MockChecker::new();
        checks
            .expect_check()
            .times(1)
            .withf(move |actual| actual == paths)
            .returning(|_| {
                Ok(CheckReport {
                    errors: vec![make_problem()],
                })
            });
        checks
    }
    fn build_inspection() -> MockInspector {
        let mut inspections = MockInspector::new();
        inspections
            .expect_inspect()
            .times(1)
            .withf(|ids| ids == [ElementAddress::parse("x.md").unwrap()])
            .returning(|_| {
                Ok(Inspection {
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
                })
            });
        inspections
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
        let exit = write_response(&mut output, response);
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
