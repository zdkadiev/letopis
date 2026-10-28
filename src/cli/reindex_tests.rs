use super::{ResponseResult, finish_foreground, parse_request, write_response};
use crate::service::{CommandErrorKind, CommandProblem, MockBackgroundRunner};
use serde_json::{Value, json};

#[test]
fn accepts_only_empty_reindex_input() {
    // Given / When / Then.
    assert!(parse_request(&["reindex".into()], b"{}".as_slice()).is_ok());
    for input in [
        "null",
        "[]",
        "{",
        "{} {}",
        r#"{"paths":[]}"#,
        r#"{"cancel":true}"#,
    ] {
        assert!(parse_request(&["reindex".into()], input.as_bytes()).is_err());
    }
    assert!(parse_request(&["reindex".into(), "--cancel".into()], b"{}".as_slice()).is_err());
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
    assert_eq!(write_response(&mut output, response), 0);
    // Then
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        json,
        json!({"ok":true,"result":{"revision":42},"warnings":[{"code":"internal_error","message":"safe launch failure","details":{}}]})
    );
    println!("CLI_RESPONSE {}", json!({"response":json,"exit":0}));
}
