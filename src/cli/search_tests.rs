//! Injected transport doubles only; real service behavior lives in binary tests.
use super::{Request, ResponseResult, finish_foreground, request_parse, response_write};
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

fn parse(body: &str) -> CommandResult<Request> {
    request_parse(&[OsString::from("search")], &mut body.as_bytes())
}
