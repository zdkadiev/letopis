use super::{ResponseResult, finish_foreground, request_parse, response_write};
use crate::service::{CommandErrorKind, CommandProblem, MockBackgroundRunner};
use serde_json::{Value, json};

#[test]
fn accepts_only_empty_reindex_input() {
    // Given: a reindex request with an empty JSON object.
    // When / Then: the empty request is accepted without selective arguments.
    assert!(request_parse(&["reindex".into()], b"{}".as_slice()).is_ok());
    // Given: malformed JSON and unsupported selective request fields.
    for input in [
        "null",
        "[]",
        "{",
        "{} {}",
        r#"{"paths":[]}"#,
        r#"{"cancel":true}"#,
    ] {
        // When / Then: each unsupported body is rejected.
        assert!(request_parse(&["reindex".into()], input.as_bytes()).is_err());
    }
    // When / Then: a cancellation flag is also rejected before any service call.
    assert!(request_parse(&["reindex".into(), "--cancel".into()], b"{}".as_slice()).is_err());
}

#[test]
fn reindex_response_carries_revision_and_launch_warning_without_inline_drain() {
    // Given: a reindex result and a background runner that reports a launch warning.
    let mut background = MockBackgroundRunner::new();
    background.expect_drain().times(0);
    background.expect_start_if_needed().times(1).returning(|| {
        Ok(Some(CommandProblem {
            code: CommandErrorKind::InternalError,
            message: "safe launch failure",
            details: Default::default(),
        }))
    });
    // When: the foreground command finishes.
    let response = finish_foreground(ResponseResult::Reindex(42), &mut background);
    let mut output = Vec::new();
    assert_eq!(response_write(&mut output, response), 0);
    // Then: one successful envelope includes the published revision and launch warning.
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        json,
        json!({"ok":true,"result":{"revision":42},"warnings":[{"code":"internal_error","message":"safe launch failure","details":{}}]})
    );
    println!("CLI_RESPONSE {}", json!({"response":json,"exit":0}));
}
