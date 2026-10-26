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
        for body in ["{}", r#"{"paths":[]}"#] {
            assert!(
                matches!(parse(&["check"], body).unwrap(), Request::Check(paths) if paths.is_empty())
            );
        }
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
            assert_eq!(
                parse(&["check"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest,
                "{body}"
            );
        }
    }

    #[test]
    fn parse_inspect_normalizes_without_resolving_membership() {
        let request = parse(&["inspect"], r#"{"element_ids":["./x.md","a/../x.md","./x.md#A","x.md#A","s.txt#L01-L02","s.txt#L1-L2"]}"#).unwrap();
        let Request::Inspect(ids) = request else {
            panic!("expected inspection");
        };
        assert_eq!(
            ids.iter().map(ElementAddress::as_str).collect::<Vec<_>>(),
            ["s.txt#L1-L2", "x.md", "x.md#A"]
        );
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
            assert_eq!(
                parse(&["inspect"], &body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn inspect_rejects_wrong_shapes_unknown_and_duplicate_fields() {
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
            assert_eq!(
                parse(&["inspect"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest,
                "{body}"
            );
        }
    }

    #[test]
    fn drain_accepts_only_eof_or_empty_object() {
        for body in ["", " \n\t", "{}"] {
            assert!(matches!(
                parse(&["embed", "--drain"], body).unwrap(),
                Request::Drain
            ));
        }
        for body in ["null", "[]", r#"{"extra":1}"#, "{} {}", "\u{b}", "\u{c}"] {
            assert_eq!(
                parse(&["embed", "--drain"], body).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn parse_argv_rejects_non_utf8_flags_and_surplus() {
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
            assert_eq!(
                parse_request(&args, &mut Unread).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }
    #[test]
    fn input_io_failure_is_redacted_invalid_request() {
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
            let error = parse_request(&args, &mut BrokenInput).unwrap_err();
            assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
            assert!(!error.to_string().contains("SECRET_SENTINEL"));
        }
    }
}

#[cfg(test)]
mod transport_tests {
    use crate::cli::handlers::{CommandResponse, HandlerServices, Handlers};
    use crate::cli::input::{Request, parse_request};
    use crate::cli::output::write_response;
    use crate::domain::{LineRange, RepoPath, Sha256, TreeOid};
    use crate::run_cli;
    use crate::service::{
        BackgroundRunner, CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem,
        CommandResult, ElementAddress, ElementLocation, InspectedElement, Inspection, Inspector,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::io::{self, Write};

    #[derive(Default)]
    struct CheckDouble {
        calls: Vec<Vec<RepoPath>>,
        fail: bool,
    }
    impl Checker for CheckDouble {
        fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
            self.calls.push(paths.to_vec());
            if self.fail {
                return Err(CommandError::new_with_cause(
                    CommandErrorKind::ValidationFailed,
                    "safe failure",
                    io::Error::other("SECRET_SENTINEL"),
                ));
            }
            Ok(CheckReport {
                errors: vec![make_problem()],
            })
        }
    }
    #[derive(Default)]
    struct BackgroundDouble {
        starts: usize,
        drains: usize,
        warn: bool,
        fail: bool,
    }
    impl BackgroundRunner for BackgroundDouble {
        fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>> {
            self.starts += 1;
            if self.fail {
                return Err(CommandError::new(
                    CommandErrorKind::InternalError,
                    "safe background failure",
                ));
            }
            Ok(self.warn.then(make_problem))
        }
        fn drain(&mut self) -> CommandResult<()> {
            self.drains += 1;
            Ok(())
        }
    }
    #[derive(Default)]
    struct InspectDouble {
        calls: Vec<Vec<ElementAddress>>,
    }
    impl Inspector for InspectDouble {
        fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection> {
            self.calls.push(ids.to_vec());
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
        }
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
    #[test]
    fn check_dispatches_once_and_findings_remain_successful() {
        let args = ["check".into()];
        let request = parse_request(
            &args,
            &mut br#"{"paths":["./a.md","b/../a.md","B.md"]}"#.as_slice(),
        )
        .unwrap();
        let mut checks = CheckDouble::default();
        let mut background = BackgroundDouble::default();
        let response = Handlers::new(HandlerServices {
            checks: Some(&mut checks),
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(request);
        let (exit, value) = render(response);
        assert_eq!(
            checks.calls,
            [vec![
                RepoPath::parse("B.md").unwrap(),
                RepoPath::parse("a.md").unwrap()
            ]]
        );
        assert_eq!((background.starts, background.drains), (1, 0));
        assert_eq!(exit, 0);
        assert_eq!(
            value,
            json!({"ok":true,"result":{"errors":[{"code":"validation_failed","message":"safe finding","details":{"path":"a.md"}}]}})
        );
    }
    #[test]
    fn inspection_renders_only_metadata_and_optional_lines() {
        let mut inspections = InspectDouble::default();
        let ids = vec![ElementAddress::parse("x.md").unwrap()];
        let mut background = BackgroundDouble {
            warn: true,
            ..BackgroundDouble::default()
        };
        let response = Handlers::new(HandlerServices {
            inspections: Some(&mut inspections),
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(Request::Inspect(ids.clone()));
        let (exit, value) = render(response);
        assert_eq!(inspections.calls, [ids]);
        assert_eq!(background.starts, 1);
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
    fn unavailable_routes_fail_without_starting_background() {
        let mut background = BackgroundDouble::default();
        for request in [
            Request::Check(vec![]),
            Request::Inspect(vec![ElementAddress::parse("x.md").unwrap()]),
        ] {
            let error = Handlers::new(HandlerServices {
                background: Some(&mut background),
                ..HandlerServices::default()
            })
            .execute(request)
            .unwrap_err();
            assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
            assert_eq!(error.get_message(), "operation unavailable in this build");
        }
        let error = Handlers::new(HandlerServices {
            ..HandlerServices::default()
        })
        .execute(Request::Drain)
        .unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!((background.starts, background.drains), (0, 0));
    }
    #[test]
    fn drain_delegates_once_without_recursion() {
        let mut background = BackgroundDouble::default();
        let response = Handlers::new(HandlerServices {
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(Request::Drain);
        assert_eq!(render(response), (0, json!({"ok":true,"result":{}})));
        assert_eq!((background.starts, background.drains), (0, 1));
    }
    #[test]
    fn failed_service_does_not_start_background_or_disclose_cause() {
        let mut checks = CheckDouble {
            fail: true,
            ..CheckDouble::default()
        };
        let mut background = BackgroundDouble::default();
        let response = Handlers::new(HandlerServices {
            checks: Some(&mut checks),
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(Request::Check(vec![]));
        assert_eq!(
            render(response),
            (
                1,
                json!({"ok":false,"error":{"code":"validation_failed","message":"safe failure","details":{}}})
            )
        );
        assert_eq!(checks.calls.len(), 1);
        assert_eq!(background.starts, 0);
    }
    #[test]
    fn background_readiness_error_remains_failure() {
        let mut checks = CheckDouble::default();
        let mut background = BackgroundDouble {
            fail: true,
            ..BackgroundDouble::default()
        };
        let error = Handlers::new(HandlerServices {
            checks: Some(&mut checks),
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(Request::Check(vec![]))
        .unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(background.starts, 1);
    }
    #[test]
    fn render_every_catalogue_error_without_causes() {
        for (kind, code) in [
            (CommandErrorKind::InvalidRequest, "invalid_request"),
            (CommandErrorKind::NotFound, "not_found"),
            (CommandErrorKind::ValidationFailed, "validation_failed"),
            (CommandErrorKind::EmbeddingFailed, "embedding_failed"),
            (CommandErrorKind::InternalError, "internal_error"),
            (CommandErrorKind::Cancelled, "cancelled"),
        ] {
            let error = CommandError::new_with_cause(
                kind,
                "safe failure",
                io::Error::other("SECRET_SENTINEL"),
            );
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
        for (remaining, expected_calls) in [(0, 1), (5, 2)] {
            let mut writer = FailingWriter {
                remaining,
                calls: 0,
                flushes: 0,
                bytes: vec![],
                fail_flush: false,
            };
            let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
            assert_eq!(write_response(&mut writer, Err(error)), 1);
            assert_eq!(writer.calls, expected_calls);
            assert_eq!(writer.bytes.len(), remaining);
            assert_eq!(writer.flushes, 0);
        }
    }
    #[test]
    fn flush_failure_returns_one_after_one_complete_envelope() {
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            fail_flush: true,
        };
        let error = CommandError::new(CommandErrorKind::InternalError, "safe failure");
        assert_eq!(write_response(&mut writer, Err(error)), 1);
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes.last(), Some(&b'\n'));
        assert!(serde_json::from_slice::<Value>(&writer.bytes).is_ok());
    }
    #[test]
    fn missing_checker_uses_one_safe_error_envelope() {
        let mut output = vec![];
        let response = Handlers::new(HandlerServices {
            ..HandlerServices::default()
        })
        .execute(Request::Check(vec![]));
        let exit = write_response(&mut output, response);
        assert_eq!(exit, 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap(),
            json!({"ok":false,"error":{"code":"internal_error","message":"operation unavailable in this build","details":{}}})
        );
    }
    #[test]
    fn success_writes_one_complete_envelope_and_flushes_once() {
        let mut background = BackgroundDouble::default();
        let response = Handlers::new(HandlerServices {
            background: Some(&mut background),
            ..HandlerServices::default()
        })
        .execute(Request::Drain);
        let mut writer = FailingWriter {
            remaining: usize::MAX,
            calls: 0,
            flushes: 0,
            bytes: vec![],
            fail_flush: false,
        };
        assert_eq!(write_response(&mut writer, response), 0);
        assert_eq!((writer.calls, writer.flushes), (1, 1));
        assert_eq!(writer.bytes, b"{\"ok\":true,\"result\":{}}\n");
    }
    #[test]
    fn check_without_background_still_succeeds_without_warnings() {
        let mut checks = CheckDouble::default();
        let (exit, value) = render(
            Handlers::new(HandlerServices {
                checks: Some(&mut checks),
                ..HandlerServices::default()
            })
            .execute(Request::Check(vec![])),
        );
        assert_eq!(exit, 0);
        assert_eq!(checks.calls, [Vec::<RepoPath>::new()]);
        assert!(value.get("warnings").is_none());
    }
    #[test]
    fn run_cli_rejects_invalid_input() {
        for (args, body, code) in [
            (vec!["check".into()], "null", "invalid_request"),
            (vec!["inspect".into()], "{}", "invalid_request"),
        ] {
            let mut output = vec![];
            assert_eq!(run_cli(args, &mut body.as_bytes(), &mut output), 1);
            assert_eq!(
                serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
                code
            );
            assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        }
    }
}
