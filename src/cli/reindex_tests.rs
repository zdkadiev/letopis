use super::{HandlerServices, Handlers, parse_request, write_response};
use crate::service::{
    BackgroundRunner, CommandErrorKind, CommandProblem, CommandResult, DocumentResult,
    RefreshOutcome, Refresher,
};
use serde_json::{Value, json};
use std::cell::Cell;

#[test]
fn accepts_only_empty_reindex_input() {
    // Arrange / Act / Assert.
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
    // Arrange.
    let calls = Cell::new(0);
    let mut documents = Documents(&calls);
    let mut background = Background;
    let request = parse_request(&["reindex".into()], b"{}".as_slice()).unwrap();
    // Act.
    let response = Handlers::new(HandlerServices {
        refresher: Some(&mut documents),
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request);
    let mut output = Vec::new();
    assert_eq!(write_response(&mut output, response), 0);
    // Assert.
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        json,
        json!({"ok":true,"result":{"revision":42},"warnings":[{"code":"internal_error","message":"safe launch failure","details":{}}]})
    );
    assert_eq!(calls.get(), 1);
    println!("CLI_RESPONSE {}", json!({"response":json,"exit":0}));
}

struct Documents<'a>(&'a Cell<usize>);
struct Background;
impl Refresher for Documents<'_> {
    fn reindex(&mut self) -> DocumentResult<RefreshOutcome> {
        self.0.set(self.0.get() + 1);
        Ok(RefreshOutcome {
            revision: 42,
            published: true,
            files_updated: 1,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    }
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        panic!("reindex must not call incremental refresh")
    }
}
impl BackgroundRunner for Background {
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>> {
        Ok(Some(CommandProblem {
            code: CommandErrorKind::InternalError,
            message: "safe launch failure",
            details: Default::default(),
        }))
    }
    fn drain(&mut self) -> CommandResult<()> {
        panic!("reindex must not wait for inference")
    }
}
