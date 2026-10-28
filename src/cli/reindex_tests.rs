use super::{execute_reindex, parse_request, write_response};
use crate::service::{
    CommandErrorKind, CommandProblem, MockBackgroundRunner, MockRefresher, RefreshOutcome,
};
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
fn reindex_dispatches_full_operation_without_refresh_or_inline_drain() {
    // Given
    let mut documents = MockRefresher::new();
    documents.expect_refresh().times(0);
    documents.expect_reindex().times(1).returning(|| {
        Ok(RefreshOutcome {
            revision: 42,
            published: true,
            files_updated: 1,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    });
    let mut background = MockBackgroundRunner::new();
    background.expect_drain().times(0);
    background.expect_start_if_needed().times(1).returning(|| {
        Ok(Some(CommandProblem {
            code: CommandErrorKind::InternalError,
            message: "safe launch failure",
            details: Default::default(),
        }))
    });
    // When
    let response = execute_reindex(&mut documents, &mut background);
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
