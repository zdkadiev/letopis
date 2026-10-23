//! Injected transport doubles only; real service behavior lives in binary tests.
use super::{Handlers, Request, parse_request, write_response};
use crate::{
    domain::{KnowledgeId, LineRange, RepoPath},
    service::{CommandErrorKind, CommandResult, SearchFlag, SearchLink, SearchRequest, Searches},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, ffi::OsString, num::NonZeroUsize};

#[test]
fn parse_search_rejects_null_duplicate_unknown_numeric_and_trailing_inputs() {
    for body in [
        "{}",
        r#"{"query":"x"}"#,
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
        let result = parse(body);
        assert!(result.is_err(), "unexpected success: {body}");
        assert_eq!(
            result.unwrap_err().get_kind(),
            CommandErrorKind::InvalidRequest,
            "{body}"
        );
    }
    assert!(parse_request(&["search".into(), "extra".into()], &mut b"{}".as_slice()).is_err());
}

#[test]
fn malformed_source_revisions_are_rejected_before_unsupported_filters() {
    for oid in ["A".repeat(40), "a".repeat(39), "g".repeat(64)] {
        let body =
            json!({"query":"x","limit":1,"filters":{"sources":["x"],"source_revisions":[oid]}});
        assert_eq!(
            parse(&body.to_string()).unwrap_err().get_kind(),
            CommandErrorKind::InvalidRequest
        );
    }
}

#[test]
fn empty_filters_equal_absence_and_valid_restrictions_fail_closed() {
    for filters in [
        None,
        Some(json!({})),
        Some(json!({"sources":[],"source_revisions":[]})),
    ] {
        let mut body = json!({"query":"private query","limit":3});
        if let Some(filters) = filters {
            body["filters"] = filters;
        }
        let Request::Search(request) = parse(&body.to_string()).unwrap() else {
            panic!("search")
        };
        assert_eq!(request.query, "private query");
        assert_eq!(request.limit.get(), 3);
    }
    for filters in [
        json!({"needs_recheck":true}),
        json!({"needs_recheck":false}),
        json!({"sources":["source"]}),
        json!({"source_revisions":["a".repeat(40)]}),
        json!({"source_revisions":["a".repeat(64)]}),
    ] {
        let error =
            parse(&json!({"query":"x","limit":1,"filters":filters}).to_string()).unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "operation unavailable in this build");
    }
}

#[test]
fn search_handler_dispatches_typed_request_and_renders_links() {
    let mut searches = SearchDouble;
    let request = SearchRequest {
        query: "private query".into(),
        limit: NonZeroUsize::new(3).unwrap(),
    };
    let response =
        Handlers::new(None, None, Some(&mut searches), None).execute(Request::Search(request));
    let mut output = Vec::new();
    let exit = write_response(&mut output, response);
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(exit, 0);
    assert_eq!(
        value,
        json!({"ok":true,"result":[{"element_id":"x.md#a","path":"x.md","lines":[2,7],"revision":9,"flags":["stale"]}]})
    );
    println!("CLI_RESPONSE {}", json!({"exit":exit,"response":value}));
}

struct SearchDouble;
impl Searches for SearchDouble {
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>> {
        assert_eq!(request.query, "private query");
        assert_eq!(request.limit.get(), 3);
        Ok(vec![SearchLink {
            element_id: KnowledgeId::parse("x.md#a").unwrap(),
            path: RepoPath::parse("x.md").unwrap(),
            lines: LineRange::new(2, 7).unwrap(),
            revision: 9,
            flags: BTreeSet::from([SearchFlag::Stale]),
        }])
    }
}
fn parse(body: &str) -> CommandResult<Request> {
    parse_request(&[OsString::from("search")], &mut body.as_bytes())
}
