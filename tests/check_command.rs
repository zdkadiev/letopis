use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead};
mod support;

use letopis::{
    open_git_snapshots,
    service::{GitSnapshots, NodeMetadata},
};
use serde_json::{Value, json};
use std::fs;
use support::{documents::build_whole, reading::ReadingRepo};

#[test]
fn check_succeeds_with_markdown_symlink() {
    // Given: an ordinary Markdown file and a symlink candidate.
    let repo = ReadingRepo::new();
    repo.write("AGENTS.md", b"# a\n");
    std::os::unix::fs::symlink("AGENTS.md", repo.get_root().join("CLAUDE.md")).unwrap();
    // When: the real CLI checks the corpus.
    let output = repo.invoke(&["check"], b"{}");
    // Then: the corpus is accepted without validation errors.
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response, json!({"ok":true,"result":{"errors":[]}}));
}

#[test]
fn check_binary_reports_multiple_errors_and_repairs() {
    // Given: multiple invalid knowledge declarations.
    let repo = ReadingRepo::new();
    repo.write(
        "duplicate.md",
        b"# One {#one}\n<!-- letopis: {\"created_at\":1} -->\n# Again {#one}\nSECRET_RAW_BODY\n",
    );
    repo.write("nested.md", b"# Outer {#outer}\n<!-- letopis: {\"created_at\":2} -->\n## Inner {#inner}\n<!-- letopis: {\"created_at\":3} -->\nSECRET_RAW_BODY\n");
    repo.write_knowledge(
        "valid.md",
        "[target](missing.md \"letopis:type=supports;path=missing.md\")",
    );
    repo.write_knowledge("a.md", "[b](b.md \"letopis:type=includes;path=b.md\")");
    repo.write_knowledge("b.md", "[a](a.md \"letopis:type=includes;path=a.md\")");

    // When: the real check command runs.
    let response = check(&repo, b"{}");

    // Then: safe ordered findings reflect the current declarations.
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
    let storage = repo.open_storage();
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.nodes_get())
            .unwrap()
            .len(),
        3
    );
    // Given: the declaration in duplicate.md is repaired.
    repo.write_knowledge("duplicate.md", "Repaired duplicate");
    // When / Then: the real check command runs; safe ordered findings reflect the current declarations.
    assert_eq!(get_error_count(&check(&repo, b"{}")), 4);
    // Given: the declaration in nested.md is repaired.
    repo.write_knowledge("nested.md", "Repaired nesting");
    // When / Then: the real check command runs; safe ordered findings reflect the current declarations.
    assert_eq!(get_error_count(&check(&repo, b"{}")), 3);
    // Given: the declaration in missing.md is repaired.
    repo.write_knowledge("missing.md", "Target restored");
    // When / Then: the real check command runs; safe ordered findings reflect the current declarations.
    assert_eq!(get_error_count(&check(&repo, b"{}")), 2);
    // Given: the declaration in b.md is repaired.
    repo.write_knowledge("b.md", "Cycle repaired");
    // When / Then: the real check command runs; safe ordered findings reflect the current declarations.
    assert_eq!(
        check(&repo, b"{}"),
        json!({"ok":true,"result":{"errors":[]}})
    );
}

#[test]
fn check_paths_filters_owners_after_full_graph_validation() {
    // Given: graph owners and a path-scoped check request.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a/item.md", "[target](../target.md \"letopis:type=supports;path=target.md\") [other](../b/item.md \"letopis:type=includes;path=b/item.md\")");
    repo.write_knowledge("b/item.md", "[other](../a/item.md \"letopis:type=includes;path=a/item.md\") [missing](../missing.md \"letopis:type=custom7;path=missing.md\")");
    repo.write_knowledge("target.md", "Available outside selection");

    // When: the check command filters selected owners.
    let response = check(
        &repo,
        br#"{"paths":["a/item.md","./a/item.md","unused/../a/item.md","absent.md"]}"#,
    );

    // Then: full-graph validation still precedes owner filtering.
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
    let storage = repo.open_storage();
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.nodes_get())
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn check_preserves_agent_layout_and_relation_names() {
    // Given: agent-authored layout and relation names.
    let repo = ReadingRepo::new();
    let knowledge = build_whole(
        42,
        "[case](../../case.md \"letopis:type=explains_case7;path=case.md\")",
    );
    repo.write("knowledge/deep/notes.markdown", knowledge.as_bytes());
    repo.write_knowledge("case.md", "Case");
    repo.write("ordinary.md", b"Ordinary unmarked Markdown\n");
    repo.write(".gitignore", b"ignored.md\n");
    repo.write("ignored.md", b"<!-- letopis: broken -->\n");

    // When / Then: check publishes the repository corpus; authored layout and relation names remain intact.
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
    let storage = repo.open_storage();
    storage.cache.with_read_tx(|| { let view = &storage.cache;
    let nodes = view.nodes_get().unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().any(|node| matches!(&node.metadata, NodeMetadata::Knowledge(metadata) if metadata.id.as_str() == "knowledge/deep/notes.markdown" && metadata.created_at == 42)));
    assert_eq!(
        view.edges_get_by_source_id(None).unwrap()[0]
            .get_edge_type()
            .as_str(),
        "explains_case7"
    );


 Ok::<_, StorageError>(())}).unwrap();
}

#[test]
fn check_binary_rejects_invalid_input_and_infrastructure_failure() {
    // Given
    for input in [
        b"{".as_slice(),
        br#"{"paths":["../escape.md"]}"#,
        br#"{"paths":["a.md#anchor"]}"#,
    ] {
        let repo = ReadingRepo::new();
        // When
        let output = repo.invoke(&["check"], input);
        // Then
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
    // Given
    let repo = ReadingRepo::new();
    let git = open_git_snapshots(repo.get_root()).unwrap();
    let valid = git.create_tree(&[]).unwrap();
    let blob = git.write_blob(b"SECRET_SNAPSHOT_BODY").unwrap();
    let absent = "a".repeat(40);
    let foreign = "b".repeat(64);
    repo.write_knowledge(
        "missing.md",
        "[source](code.rs \"letopis:type=external;path=code.rs\")",
    );
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

    // When
    let response = check(&repo, b"{}");

    // Then
    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"absent.md","reason":"source_snapshot_unavailable","element_id":"absent.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"blob.md","reason":"source_snapshot_unavailable","element_id":"blob.md","target":blob.as_str()}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"foreign.md","reason":"source_snapshot_unavailable","element_id":"foreign.md","target":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}},
            {"code":"validation_failed","message":"knowledge source snapshot is missing","details":{"path":"missing.md","reason":"missing_source_snapshot","element_id":"missing.md"}},
            {"code":"validation_failed","message":"knowledge source is not found","details":{"path":"missing.md","reason":"source_not_found","element_id":"missing.md","target":"code.rs"}},
            {"code":"validation_failed","message":"knowledge source snapshot is unavailable","details":{"path":"repeat.md","reason":"source_snapshot_unavailable","element_id":"repeat.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
        ]}})
    );
    // Repair both root metadata and addressed bytes; an empty tree has no code.rs.
    let repaired = support::provenance::create_basis(repo.get_root(), &[("code.rs", b"source")]);
    repo.write("code.rs", b"source");
    for path in [
        "absent.md",
        "blob.md",
        "foreign.md",
        "repeat.md",
        "missing.md",
    ] {
        repo.write(path, format!("<!-- letopis: {{\"created_at\":42,\"source_snapshot\":\"{}\"}} -->\n[source](code.rs \"letopis:type=external;path=code.rs\")\n", repaired.as_str()).as_bytes());
    }
    assert_eq!(
        check(&repo, b"{}"),
        json!({"ok":true,"result":{"errors":[]}})
    );
}

#[test]
fn check_keeps_invalid_files_previous_projection_and_snapshot_findings() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("retained.md", b"<!-- letopis: {\"created_at\":42,\"source_snapshot\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"} -->\nKnowledge\n");
    // When / Then
    assert_eq!(get_error_count(&check(&repo, b"{}")), 1);
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
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "owner.md",
        "[missing](missing.md \"letopis:type=supports;path=missing.md\")",
    );

    // When
    let response = check(&repo, b"{}");

    // Then
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

fn get_error_count(response: &serde_json::Value) -> usize {
    response["result"]["errors"].as_array().unwrap().len()
}
