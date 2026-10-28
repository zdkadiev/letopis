//! Strict traversal transport; graph semantics are exercised through the real binary.
use super::{Request, execute_traverse, parse_request, write_response};
use crate::domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri};
use crate::service::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, ElementAddress, ElementLocation,
    MockBackgroundRunner, MockTraverser, Traversal, TraversalDirection, TraversalLink,
    TraversalMode, TraversalRequest,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io};

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
fn normalizes_starts_and_open_types_without_restricting_empty_types() {
    // Given
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
    // When
    let Request::Traverse(request) = parse(&body.to_string()).unwrap() else {
        panic!("traverse")
    };
    // Then
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
    // Given
    for types in [None, Some(json!([]))] {
        let mut body = build_input();
        if let Some(types) = types {
            body["edge_types"] = types;
        }
        // When
        let Request::Traverse(request) = parse(&body.to_string()).unwrap() else {
            panic!("traverse")
        };
        // Then
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
    let response = Ok(super::commands::CommandResponse {
        result: super::commands::ResponseResult::Traverse(Traversal {
            elements: vec![TraversalLink {
                element_id: ElementAddress::parse("https://Host/a?q=1#part").unwrap(),
                path: ElementLocation::Remote(SourceUri::parse("https://Host/a?q=1#part").unwrap()),
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
        let response = Ok(super::commands::CommandResponse {
            result: super::commands::ResponseResult::Traverse(Traversal {
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
fn dispatches_once_before_background_and_preserves_nonfatal_warning() {
    // Given
    let mut sequence = mockall::Sequence::new();
    let mut traversals = MockTraverser::new();
    traversals
        .expect_traverse()
        .times(1)
        .in_sequence(&mut sequence)
        .withf(matches_request)
        .returning(|_| {
            Ok(Traversal {
                elements: vec![],
                edges: None,
            })
        });
    let mut background = MockBackgroundRunner::new();
    background
        .expect_start_if_needed()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|| {
            Ok(Some(CommandProblem {
                code: CommandErrorKind::InternalError,
                message: "cannot start background embedding drain",
                details: Default::default(),
            }))
        });
    background.expect_drain().times(0);
    let request = parse_traversal(&build_input().to_string());
    // When
    let response = execute_traverse(&mut traversals, &request, &mut background);
    // Then
    assert_eq!(
        render(response),
        (
            0,
            json!({"ok":true,"result":{"elements":[]},
        "warnings":[{"code":"internal_error","message":"cannot start background embedding drain","details":{}}]})
        )
    );
}

#[test]
fn propagates_safe_failure_without_background_or_partial_result() {
    // Given
    let mut traversals = MockTraverser::new();
    traversals
        .expect_traverse()
        .times(1)
        .withf(matches_request)
        .returning(|_| {
            Err(CommandError::new_with_cause(
                CommandErrorKind::NotFound,
                "unknown traversal start",
                io::Error::other("PRIVATE_CAUSE"),
            ))
        });
    let mut background = MockBackgroundRunner::new();
    background.expect_start_if_needed().times(0);
    background.expect_drain().times(0);
    // When
    let request = parse_traversal(&build_input().to_string());
    let response = execute_traverse(&mut traversals, &request, &mut background);
    // Then
    assert_eq!(
        render(response),
        (
            1,
            json!({"ok":false,"error":{
        "code":"not_found","message":"unknown traversal start","details":{}}})
        )
    );
}

fn matches_request(request: &TraversalRequest) -> bool {
    request
        .element_ids
        .iter()
        .map(ElementAddress::as_str)
        .collect::<Vec<_>>()
        == ["a.md"]
        && (
            request.mode,
            request.direction,
            request.depth,
            request.limit.get(),
        ) == (TraversalMode::Both, TraversalDirection::Outgoing, 1, 3)
}

fn render(response: CommandResult<super::commands::CommandResponse>) -> (u8, Value) {
    let mut bytes = vec![];
    let exit = write_response(&mut bytes, response);
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
fn parse_traversal(body: &str) -> TraversalRequest {
    let Request::Traverse(request) = parse(body).unwrap() else {
        panic!("expected a traverse request");
    };
    request
}
fn parse(body: &str) -> CommandResult<Request> {
    parse_request(&["traverse".into()], body.as_bytes())
}
