#[expect(
    dead_code,
    reason = "Shared watchdog exposes unrelated CLI fixture helpers"
)]
#[path = "support/cli.rs"]
mod cli;
#[expect(
    dead_code,
    reason = "Shared drain guard exposes unrelated drain helpers"
)]
#[path = "support/drain.rs"]
mod drain;
mod support;
use letopis::{
    service::{EmbeddingExecutor, Refresher},
    wire_document_refresher, wire_embedding_executor,
};
use serde_json::{Value, json};
use std::{
    process::{Command, Output, Stdio},
    time::Duration,
};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    git::isolate_command,
    reading::ReadingRepo,
};

const MODE_CASES: [(&str, &str, &str, &str); 9] = [
    (
        "relations",
        "outgoing",
        r#"["b.md"]"#,
        r#"[["a.md","b.md","supports"]]"#,
    ),
    (
        "relations",
        "incoming",
        r#"["d.md"]"#,
        r#"[["d.md","a.md","refutes"]]"#,
    ),
    (
        "relations",
        "both",
        r#"["d.md","b.md"]"#,
        r#"[["d.md","a.md","refutes"],["a.md","b.md","supports"]]"#,
    ),
    (
        "containment",
        "outgoing",
        r#"["c.md"]"#,
        r#"[["a.md","c.md","includes"]]"#,
    ),
    ("containment", "incoming", r#"[]"#, r#"[]"#),
    (
        "containment",
        "both",
        r#"["c.md"]"#,
        r#"[["a.md","c.md","includes"]]"#,
    ),
    (
        "both",
        "outgoing",
        r#"["c.md","b.md"]"#,
        r#"[["a.md","c.md","includes"],["a.md","b.md","supports"]]"#,
    ),
    (
        "both",
        "incoming",
        r#"["d.md"]"#,
        r#"[["d.md","a.md","refutes"]]"#,
    ),
    (
        "both",
        "both",
        r#"["c.md","d.md","b.md"]"#,
        r#"[["a.md","c.md","includes"],["d.md","a.md","refutes"],["a.md","b.md","supports"]]"#,
    ),
];

#[test]
fn binary_covers_every_mode_and_direction_with_original_edges() {
    // Given
    let repo = build_graph();
    for (mode, direction, elements, edges) in MODE_CASES {
        let elements: Value = serde_json::from_str(elements).unwrap();
        let edges: Value = serde_json::from_str(edges).unwrap();
        let body = json!({"element_ids":["a.md"],"mode":mode,"direction":direction,
            "depth":1,"limit":20,"edge_limit":20});
        // When
        let response = invoke(&repo, &body, 0);
        // Then
        assert_eq!(read_ids(&response), elements, "{mode}/{direction}");
        assert_eq!(read_triples(&response), edges, "{mode}/{direction}");
        for link in response["result"]["elements"].as_array().unwrap() {
            let id = link["element_id"].as_str().unwrap();
            assert_eq!(
                *link,
                json!({"element_id":id,"path":id,"lines":if id == "d.md" { [1,3] } else { [1,2] },"revision":1,"flags":[]})
            );
        }
        assert_eq!(response.as_object().unwrap().len(), 2);
        assert_eq!(response["result"].as_object().unwrap().len(), 2);
    }
}

#[test]
fn binary_excludes_multiple_starts_and_validates_unknown_at_depth_zero() {
    // Given
    let repo = build_graph();
    let body = json!({"element_ids":["./a.md","c.md","a.md"],"mode":"both",
        "direction":"outgoing","depth":1,"limit":1});
    // When
    let response = invoke(&repo, &body, 0);
    // Then
    assert_eq!(
        response,
        json!({"ok":true,"result":{"elements":[{
        "element_id":"b.md","path":"b.md","lines":[1,2],"revision":1,"flags":[]}]}})
    );
    for depth in [0, u64::MAX] {
        let response = invoke(
            &repo,
            &json!({"element_ids":["a.md","unknown.md"],
            "mode":"both","direction":"both","depth":depth,"limit":1}),
            1,
        );
        assert_eq!(response["error"]["code"], "not_found");
        assert!(response.get("result").is_none());
    }
    assert_eq!(
        invoke(
            &repo,
            &json!({"element_ids":["a.md"],"mode":"both",
        "direction":"both","depth":0,"limit":1,"edge_limit":1}),
            0
        ),
        json!({"ok":true,"result":{"elements":[],"edges":[]}})
    );
}

#[test]
fn binary_applies_type_intersection_and_independent_caps() {
    // Given
    let repo = build_graph();
    let mut body = json!({"element_ids":["a.md"],"mode":"both","direction":"both",
        "depth":1,"limit":2,"edge_limit":1,"edge_types":[]});
    // When
    let response = invoke(&repo, &body, 0);
    // Then
    assert_eq!(read_ids(&response), json!(["c.md", "d.md"]));
    assert_eq!(
        read_triples(&response),
        json!([["a.md", "c.md", "includes"]])
    );
    body["edge_types"] = json!(["supports", "supports", "custom_type9"]);
    assert_eq!(read_ids(&invoke(&repo, &body, 0)), json!(["b.md"]));
    body["mode"] = json!("containment");
    assert_eq!(
        invoke(&repo, &body, 0),
        json!({"ok":true,"result":{"elements":[],"edges":[]}})
    );
}

#[test]
fn binary_returns_missing_ranged_and_opaque_sources_without_content() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "private knowledge\n[m](missing.rs \"letopis:type=external;path=missing.rs\")\n[r](src.rs#L2-L3 \"letopis:type=external;path=src.rs#L2-L3\")\n[u](https://Host/a?q=1#part \"letopis:type=external;path=https://Host/a?q=1#part\")");
    repo.write("src.rs", b"private source\nsecond\n");
    let body = json!({"element_ids":["a.md"],"mode":"relations","direction":"outgoing",
        "depth":1,"limit":10});
    // When
    let response = invoke(&repo, &body, 0);
    // Then
    assert_eq!(
        response,
        json!({"ok":true,"result":{"elements":[
            {"element_id":"https://Host/a?q=1#part","path":"https://Host/a?q=1#part","revision":1,"flags":[]},
            {"element_id":"missing.rs","path":"missing.rs","revision":1,"flags":["source_not_found"]},
            {"element_id":"src.rs#L2-L3","path":"src.rs","lines":[2,3],"revision":1,"flags":["source_not_found"]}
        ]}})
    );
    let mut capped = body;
    capped["limit"] = json!(1);
    assert_eq!(
        read_ids(&invoke(&repo, &capped, 0)),
        json!(["https://Host/a?q=1#part"])
    );
    let incoming = invoke(
        &repo,
        &json!({"element_ids":["https://Host/a?q=1#part"],
        "mode":"both","direction":"incoming","depth":1,"limit":1,"edge_limit":1}),
        0,
    );
    assert_eq!(read_ids(&incoming), json!(["a.md"]));
    assert_eq!(
        read_triples(&incoming),
        json!([["a.md", "https://Host/a?q=1#part", "external"]])
    );
    repo.write_knowledge("a.md", "links removed");
    assert_eq!(
        invoke(
            &repo,
            &json!({"element_ids":["missing.rs"],"mode":"both",
        "direction":"both","depth":0,"limit":1}),
            1
        )["error"]["code"],
        "not_found"
    );
}

#[test]
fn binary_uses_no_inference_with_fully_seeded_backlog() {
    // Given
    let repo = build_graph();
    let http = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.seed_vectors(
        &config,
        &[
            ("a.md", &[1.0, 0.0]),
            ("b.md", &[1.0, 0.0]),
            ("c.md", &[1.0, 0.0]),
            ("d.md", &[1.0, 0.0]),
        ],
    );
    // When / Then
    assert!(
        !wire_embedding_executor(repo.get_root())
            .unwrap()
            .has_work()
            .unwrap()
    );
    let response = invoke(
        &repo,
        &json!({"element_ids":["a.md"],"mode":"both",
        "direction":"outgoing","depth":1,"limit":1}),
        0,
    );
    assert_eq!(read_ids(&response), json!(["c.md"]));
    assert!(http.take_requests().is_empty());
    assert!(
        !wire_embedding_executor(repo.get_root())
            .unwrap()
            .has_work()
            .unwrap()
    );
}

#[test]
fn nested_binary_discovers_root_and_launches_existing_detached_drain() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "private query is never embedded");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    repo.write_config("ollama", &http.get_base_url(), "fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    command
        .current_dir(repo.get_root().join("subdir"))
        .arg("traverse")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cli::OwnedChild::spawn(&mut command);
    let mut owner = drain::DetachedOwner::new(repo.get_root(), child.get_id());
    child.send_input(
        br#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":0,"limit":1}"#,
    );
    // When
    let response = read_response(&child.collect_output(Duration::from_secs(10)), 0);
    barrier.wait_until_entered();
    // Then
    assert!(owner.read_pid() > 0);
    barrier.release();
    owner.wait_for_exit();
    assert_eq!(response, json!({"ok":true,"result":{"elements":[]}}));
    assert!(repo.get_root().join(".letopis/cache.db").is_file());
    assert!(!repo.get_root().join("subdir/.letopis").exists());
    assert_eq!(
        http.take_requests(),
        vec![json!({"model":"fixture",
        "input":["private query is never embedded"],"truncate":false})]
    );
}

fn build_graph() -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "private a\n[b](b.md \"letopis:type=supports;path=b.md\")\n[c](c.md \"letopis:type=includes;path=c.md\")",
    );
    repo.write_knowledge("b.md", "private b");
    repo.write_knowledge("c.md", "private c");
    repo.write_knowledge(
        "d.md",
        "private d\n[a](a.md \"letopis:type=refutes;path=a.md\")",
    );
    repo
}
fn invoke(repo: &ReadingRepo, body: &Value, exit: i32) -> Value {
    read_response(
        &repo.invoke(&["traverse"], body.to_string().as_bytes()),
        exit,
    )
}
fn read_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn read_ids(response: &Value) -> Value {
    Value::Array(
        response["result"]["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|link| link["element_id"].clone())
            .collect(),
    )
}
fn read_triples(response: &Value) -> Value {
    Value::Array(
        response["result"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| json!([edge["source"], edge["target"], edge["type"]]))
            .collect(),
    )
}
