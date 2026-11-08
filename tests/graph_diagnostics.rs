mod support;

use letopis::{
    domain::LineRange,
    open_cache_storage,
    service::{
        CacheDatabase, CommandErrorKind, CommandProblem, GraphDiagnostics, NodeMetadata, Refresher,
        StorageRead, StorageResult,
    },
    wire_document_refresher, wire_graph_diagnostics,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{error::Error, fs, rc::Rc};
use support::documents::{DocumentRepo, build_whole};

const RAW_BODY: &str = "SECRET_RAW_BODY";
const ANCHORED: &str = "# One {#one}\n<!-- letopis: {\"created_at\":42} -->\nAnchored body\n";

#[test]
fn reports_current_file_and_graph_problems_until_each_cause_is_repaired() {
    // Given
    let repo = DocumentRepo::new();
    let source = build_whole(
        7,
        "[target](missing.md \"letopis:type=supports;path=missing.md\")",
    );
    let invalid = build_whole(
        8,
        &format!("{RAW_BODY} [self](bad.md \"letopis:type=includes;path=bad.md\")"),
    );
    repo.write("a.md", &source);
    repo.write("bad.md", &invalid);
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();

    documents.refresh(&[]).unwrap();
    // When
    let problems = diagnose(&repo);

    // Then
    assert_findings(
        &problems,
        &json!([
            {"message":"knowledge target is missing", "details":{
                "path":"a.md", "reason":"dangling_target", "element_id":"a.md", "target":"missing.md"}},
            {"message":"knowledge cannot link to itself", "details":{"path":"bad.md", "reason":"self_loop"}}
        ]),
    );
    assert_bytes(&repo, "a.md", &source);
    assert_bytes(&repo, "bad.md", &invalid);

    // A new target repairs only the derived graph problem; the source is untouched.
    let target = build_whole(9, "Target appears");
    repo.write("missing.md", &target);
    documents.refresh(&[]).unwrap();
    let problems = diagnose(&repo);
    assert_findings(
        &problems,
        &json!([
            {"message":"knowledge cannot link to itself", "details":{"path":"bad.md", "reason":"self_loop"}}
        ]),
    );
    assert_bytes(&repo, "a.md", &source);
    assert_bytes(&repo, "bad.md", &invalid);
    assert_bytes(&repo, "missing.md", &target);

    let corrected = build_whole(8, "Corrected body");
    repo.write("bad.md", &corrected);
    documents.refresh(&[]).unwrap();
    assert!(diagnose(&repo).is_empty());
    assert_bytes(&repo, "a.md", &source);
    assert_bytes(&repo, "bad.md", &corrected);
    assert_bytes(&repo, "missing.md", &target);
}

#[test]
fn preserves_anchor_identity_and_author_time_when_prefix_lines_move_coordinates() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("a.md", ANCHORED);
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    // When
    let before = repo.get_state();
    let NodeMetadata::Knowledge(original) = &before.get_node("a.md#one").metadata else {
        panic!("expected anchored knowledge");
    };
    // Then
    assert_eq!(original.id.as_str(), "a.md#one");
    assert_eq!(original.created_at, 42);
    assert_eq!(original.lines, LineRange::new(1, 3).unwrap());

    let edited =
        format!("Ordinary first line\nOrdinary second line\nOrdinary third line\n{ANCHORED}");
    repo.write("a.md", &edited);
    documents.refresh(&[]).unwrap();
    let problems = diagnose(&repo);
    let after = repo.get_state();

    let NodeMetadata::Knowledge(moved) = &after.get_node("a.md#one").metadata else {
        panic!("expected anchored knowledge");
    };
    assert_eq!(after.nodes.len(), 1);
    assert_eq!(moved.id.as_str(), "a.md#one");
    assert_eq!(moved.created_at, 42);
    assert_eq!(moved.lines, LineRange::new(4, 6).unwrap());
    assert_eq!(
        before.get_node("a.md#one").row_id,
        after.get_node("a.md#one").row_id
    );
    assert!(problems.is_empty());
    assert_bytes(&repo, "a.md", &edited);
}

#[test]
fn isolates_duplicate_and_nested_anchor_errors_from_a_valid_file() {
    // Given
    let repo = DocumentRepo::new();
    let duplicate =
        "# One {#one}\n<!-- letopis: {\"created_at\":1} -->\n# Again {#one}\nSECRET_RAW_BODY\n";
    let nested = "# Outer {#outer}\n<!-- letopis: {\"created_at\":2} -->\n## Inner {#inner}\n<!-- letopis: {\"created_at\":3} -->\nSECRET_RAW_BODY\n";
    let valid = build_whole(
        4,
        "[missing](missing.md \"letopis:type=supports;path=missing.md\")",
    );
    repo.write("duplicate.md", duplicate);
    repo.write("nested.md", nested);
    repo.write("valid.md", &valid);
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();

    let outcome = documents.refresh(&[]).unwrap();
    // When
    let problems = diagnose(&repo);
    let state = repo.get_state();

    // Then
    assert_eq!(outcome.invalid_files_updated, 2);
    assert_eq!(state.nodes.len(), 1);
    assert!(state.get_file("valid.md").valid);
    assert!(!state.get_file("duplicate.md").valid);
    assert!(!state.get_file("nested.md").valid);
    assert_findings(
        &problems,
        &json!([
            {"message":"duplicate explicit anchor", "details":{"path":"duplicate.md", "reason":"duplicate_anchor"}},
            {"message":"knowledge elements cannot be nested", "details":{"path":"nested.md", "reason":"nested_element"}},
            {"message":"knowledge target is missing", "details":{
                "path":"valid.md", "reason":"dangling_target", "element_id":"valid.md", "target":"missing.md"}}
        ]),
    );
    assert_bytes(&repo, "duplicate.md", duplicate);
    assert_bytes(&repo, "nested.md", nested);
    assert_bytes(&repo, "valid.md", &valid);
}

#[test]
fn diagnoses_the_pinned_revision_after_another_connection_publishes_a_target() {
    // Given: a reader pinned before a missing target is published.
    let repo = DocumentRepo::new();
    let source = build_whole(
        1,
        "[target](missing.md \"letopis:type=includes;path=missing.md\")",
    );
    repo.write("a.md", &source);
    // The refresher owns the writer connection before the separate reader pins.
    let mut writer = wire_document_refresher(repo.get_root()).unwrap();
    // When / Then: diagnostics inspect the pinned graph revision; later publication does not change pinned findings.
    assert_eq!(writer.refresh(&[]).unwrap().revision, 1);
    let reader = Rc::new(
        open_cache_storage(
            &repo.get_root().join(".letopis/cache.db"),
            &repo.get_root().join(".letopis/vectors.db"),
        )
        .unwrap(),
    );
    let diagnostics = wire_graph_diagnostics(Rc::clone(&reader));
    reader.with_read_tx(|| -> StorageResult<()> {
    let old = reader.as_ref();
    assert_eq!(old.project_get_by_id().unwrap(), 1);
    assert_eq!(
        diagnostics.find_problems(&[]).unwrap().len(),
        1
    );

    repo.write("missing.md", build_whole(2, "Published target"));
    let publication = writer.refresh(&[]).unwrap();
    let problems = diagnostics.find_problems(&[]).unwrap();

    assert_eq!(publication.revision, 2);
    assert!(publication.is_published);
    assert_eq!(old.project_get_by_id().unwrap(), 1);
    assert_findings(
        &problems,
        &json!([
            {"message":"knowledge target is missing", "details":{
                "path":"a.md", "reason":"dangling_target", "element_id":"a.md", "target":"missing.md"}}
        ]),
    );
    Ok(())
    }).unwrap();
    assert_eq!(
        reader.with_read_tx(|| reader.project_get_by_id()).unwrap(),
        2
    );
    assert!(diagnostics.find_problems(&[]).unwrap().is_empty());
    assert_bytes(&repo, "a.md", &source);
}

#[test]
fn propagates_corrupt_edge_reads_instead_of_returning_clean_or_partial_findings() {
    // Given: corrupt persisted graph edges.
    let repo = DocumentRepo::new();
    let invalid = build_whole(
        1,
        &format!("{RAW_BODY} [self](bad.md \"letopis:type=includes;path=bad.md\")"),
    );
    repo.write("bad.md", &invalid);
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let cache = repo.get_root().join(".letopis/cache.db");
    let storage =
        Rc::new(open_cache_storage(&cache, &repo.get_root().join(".letopis/vectors.db")).unwrap());
    storage
        .with_read_tx(|| -> StorageResult<()> {
            // When: graph diagnostics retrieve those edges.
            let before = storage.as_ref();
            // Then: the read error propagates without a partial clean result.
            assert_eq!(before.files_get().unwrap().len(), 1);
            assert!(!before.files_get().unwrap()[0].valid);
            Ok(())
        })
        .unwrap();
    // Deliberately corrupt only this private temporary cache before opening a read.
    Connection::open(&cache)
        .unwrap()
        .execute("drop table edges", [])
        .unwrap();

    let error = wire_graph_diagnostics(storage)
        .find_problems(&[])
        .unwrap_err();

    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.to_string(), "cannot read graph diagnostics");
    assert!(error.source().is_some());
    let public = format!("{error} {error:?}");
    for private in [
        RAW_BODY,
        "drop table",
        "no such table",
        "edges",
        cache.to_str().unwrap(),
    ] {
        assert!(
            !public.contains(private),
            "exposed private payload: {public}"
        );
    }
    assert_bytes(&repo, "bad.md", &invalid);
}

fn diagnose(repo: &DocumentRepo) -> Vec<CommandProblem> {
    let storage = Rc::new(
        open_cache_storage(
            &repo.get_root().join(".letopis/cache.db"),
            &repo.get_root().join(".letopis/vectors.db"),
        )
        .unwrap(),
    );
    wire_graph_diagnostics(storage).find_problems(&[]).unwrap()
}

fn assert_findings(problems: &[CommandProblem], expected: &Value) {
    assert!(
        problems
            .iter()
            .all(|problem| problem.code == CommandErrorKind::ValidationFailed)
    );
    let actual: Vec<Value> = problems
        .iter()
        .map(|problem| json!({"message":problem.message, "details":problem.details}))
        .collect();
    assert_eq!(&Value::Array(actual), expected);
    assert!(!format!("{problems:?}").contains(RAW_BODY));
}

fn assert_bytes(repo: &DocumentRepo, path: &str, expected: &str) {
    assert_eq!(
        fs::read(repo.get_root().join(path)).unwrap(),
        expected.as_bytes()
    );
}
