use super::{IndexMode, Request, request_parse};
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
