mod support;

use letopis::{
    open_git_snapshots,
    service::{GitSnapshots, NodeMetadata, Storage, StorageRead},
};
use serde_json::{Value, json};
use std::fs;
use support::{documents::build_whole, reading::ReadingRepo};

#[test]
fn check_binary_reports_multiple_errors_and_repairs() {
    let repo = ReadingRepo::new();
    repo.write(
        "duplicate.md",
        b"# One {#one}\n<!-- letopis: {\"created_at\":1} -->\n# Again {#one}\nSECRET_RAW_BODY\n",
    );
    repo.write("nested.md", b"# Outer {#outer}\n<!-- letopis: {\"created_at\":2} -->\n## Inner {#inner}\n<!-- letopis: {\"created_at\":3} -->\nSECRET_RAW_BODY\n");
    repo.write_knowledge("valid.md", "[target](missing.md \"letopis:type=supports\")");
    repo.write_knowledge("a.md", "[b](b.md \"letopis:type=includes\")");
    repo.write_knowledge("b.md", "[a](a.md \"letopis:type=includes\")");

    let response = check(&repo, b"{}");

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"knowledge participates in an includes cycle","details":{"path":"a.md","reason":"includes_cycle","element_id":"a.md"}},
            {"code":"validation_failed","message":"knowledge participates in an includes cycle","details":{"path":"b.md","reason":"includes_cycle","element_id":"b.md"}},
            {"code":"validation_failed","message":"duplicate explicit anchor","details":{"path":"duplicate.md","reason":"duplicate_anchor"}},
            {"code":"validation_failed","message":"knowledge elements cannot be nested","details":{"path":"nested.md","reason":"nested_element"}},
            {"code":"validation_failed","message":"knowledge target is missing","details":{"path":"valid.md","reason":"dangling_target","element_id":"valid.md","target":"missing.md"}}
        ]}})
    );
    let mut storage = repo.open_storage();
    assert_eq!(storage.begin_read().unwrap().list_nodes().unwrap().len(), 3);
    repo.write_knowledge("duplicate.md", "Repaired duplicate");
    assert_eq!(
        check(&repo, b"{}")["result"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    repo.write_knowledge("nested.md", "Repaired nesting");
    assert_eq!(
        check(&repo, b"{}")["result"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    repo.write_knowledge("missing.md", "Target restored");
    assert_eq!(
        check(&repo, b"{}")["result"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    repo.write_knowledge("b.md", "Cycle repaired");
    assert_eq!(
        check(&repo, b"{}"),
        json!({"ok":true,"result":{"errors":[]}})
    );
}

#[test]
fn check_paths_filters_owners_after_full_graph_validation() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a/item.md", "[target](target.md \"letopis:type=supports\") [other](b/item.md \"letopis:type=includes\")");
    repo.write_knowledge("b/item.md", "[other](a/item.md \"letopis:type=includes\") [missing](missing.md \"letopis:type=custom7\")");
    repo.write_knowledge("target.md", "Available outside selection");

    let response = check(
        &repo,
        br#"{"paths":["a/item.md","./a/item.md","unused/../a/item.md","absent.md"]}"#,
    );

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"knowledge participates in an includes cycle","details":{"path":"a/item.md","reason":"includes_cycle","element_id":"a/item.md"}}
        ]}})
    );
    assert_eq!(
        check(&repo, br#"{"paths":["target.md","absent.md"]}"#),
        json!({"ok":true,"result":{"errors":[]}})
    );
    let mut storage = repo.open_storage();
    assert_eq!(storage.begin_read().unwrap().list_nodes().unwrap().len(), 3);
}

#[test]
fn check_preserves_agent_layout_and_relation_names() {
    let repo = ReadingRepo::new();
    let knowledge = build_whole(42, "[case](case.md \"letopis:type=explains_case7\")");
    repo.write("knowledge/deep/notes.markdown", knowledge.as_bytes());
    repo.write_knowledge("case.md", "Case");
    repo.write("ordinary.md", b"Ordinary unmarked Markdown\n");
    repo.write(".gitignore", b"ignored.md\n");
    repo.write("ignored.md", b"<!-- letopis: broken -->\n");

    assert_eq!(
        check(&repo, b"{}"),
        json!({"ok":true,"result":{"errors":[]}})
    );

    for (path, expected) in [
        ("knowledge/deep/notes.markdown", knowledge.as_bytes()),
        ("ordinary.md", b"Ordinary unmarked Markdown\n"),
        ("ignored.md", b"<!-- letopis: broken -->\n"),
    ] {
        assert_eq!(fs::read(repo.get_root().join(path)).unwrap(), expected);
    }
    let mut storage = repo.open_storage();
    let mut view = storage.begin_read().unwrap();
    let nodes = view.list_nodes().unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().any(|node| matches!(&node.metadata, NodeMetadata::Knowledge(metadata) if metadata.id.as_str() == "knowledge/deep/notes.markdown" && metadata.created_at == 42)));
    assert_eq!(
        view.list_edges(None).unwrap()[0].get_edge_type().as_str(),
        "explains_case7"
    );
}

#[test]
fn check_binary_rejects_invalid_input_and_infrastructure_failure() {
    for input in [
        b"{".as_slice(),
        br#"{"paths":["../escape.md"]}"#,
        br#"{"paths":["a.md#anchor"]}"#,
    ] {
        let repo = ReadingRepo::new();
        let output = repo.invoke(&["check"], input);
        assert_eq!(output.status.code(), Some(1));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(!repo.get_root().join(".letopis").exists());
    }
    let repo = ReadingRepo::new();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("sentinel"), b"untouched").unwrap();
    std::os::unix::fs::symlink(external.path(), repo.get_root().join(".letopis")).unwrap();
    let output = repo.invoke(&["check"], b"{}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "internal_error");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(external.path().to_str().unwrap()));
    assert_eq!(
        fs::read(external.path().join("sentinel")).unwrap(),
        b"untouched"
    );
    assert_eq!(fs::read_dir(external.path()).unwrap().count(), 1);
}

#[test]
fn check_snapshot_findings_are_current_and_safe() {
    let repo = ReadingRepo::new();
    let git = open_git_snapshots(repo.get_root()).unwrap();
    let valid = git.create_tree(&[]).unwrap();
    let blob = git.write_blob(b"SECRET_SNAPSHOT_BODY").unwrap();
    let absent = "a".repeat(40);
    let foreign = "b".repeat(64);
    repo.write_knowledge("missing.md", "[source](code.rs \"letopis:type=external\")");
    for (path, oid) in [
        ("valid.md", valid.as_str()),
        ("absent.md", absent.as_str()),
        ("blob.md", blob.as_str()),
        ("foreign.md", foreign.as_str()),
        ("repeat.md", absent.as_str()),
    ] {
        repo.write(
            path,
            format!(
                "<!-- letopis: {{\"created_at\":42,\"source_snapshot\":\"{oid}\"}} -->\nKnowledge\n"
            )
            .as_bytes(),
        );
    }

    let response = check(&repo, b"{}");

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"absent.md","reason":"source_snapshot_unavailable","element_id":"absent.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"blob.md","reason":"source_snapshot_unavailable","element_id":"blob.md","target":blob.as_str()}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"foreign.md","reason":"source_snapshot_unavailable","element_id":"foreign.md","target":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}},
            {"code":"validation_failed","message":"knowledge source snapshot is missing","details":{"path":"missing.md","reason":"missing_source_snapshot","element_id":"missing.md"}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"repeat.md","reason":"source_snapshot_unavailable","element_id":"repeat.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
        ]}})
    );
    for path in [
        "absent.md",
        "blob.md",
        "foreign.md",
        "repeat.md",
        "missing.md",
    ] {
        repo.write(path, format!("<!-- letopis: {{\"created_at\":42,\"source_snapshot\":\"{}\"}} -->\n[source](code.rs \"letopis:type=external\")\n", valid.as_str()).as_bytes());
    }
    assert_eq!(
        check(&repo, b"{}"),
        json!({"ok":true,"result":{"errors":[]}})
    );
}

#[test]
fn check_keeps_invalid_files_previous_projection_and_snapshot_findings() {
    let repo = ReadingRepo::new();
    repo.write("retained.md", b"<!-- letopis: {\"created_at\":42,\"source_snapshot\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"} -->\nKnowledge\n");
    assert_eq!(
        check(&repo, b"{}")["result"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    repo.write("retained.md", b"<!-- letopis: broken SECRET_RAW_BODY -->\n");

    let response = check(&repo, b"{}");

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"invalid letopis metadata","details":{"path":"retained.md","reason":"invalid_metadata"}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"retained.md","reason":"source_snapshot_unavailable","element_id":"retained.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
        ]}})
    );
}

#[test]
fn check_binary_reports_literal_single_dangling_target() {
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "owner.md",
        "[missing](missing.md \"letopis:type=supports\")",
    );

    let response = check(&repo, b"{}");

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"knowledge target is missing","details":{"path":"owner.md","reason":"dangling_target","element_id":"owner.md","target":"missing.md"}}
        ]}})
    );
}

fn check(repo: &ReadingRepo, input: &[u8]) -> Value {
    let output = repo.invoke(&["check"], input);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET_RAW_BODY"));
    response
}
