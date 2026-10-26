mod support;

use letopis::domain::TreeOid;
use serde_json::{Value, json};
use std::fs;
use support::{
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

#[test]
fn search_observes_source_only_changes_without_new_cache_revision() {
    // Given
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("code.rs", b"old\n")]);
    repo.write("code.rs", b"old\n");
    repo.write(
        "note.md",
        build_note(Some(&basis), &["code.rs"], "needle").as_bytes(),
    );
    let initial = search(&repo);
    assert_eq!(initial[0]["flags"], json!([]));
    repo.write("code.rs", b"new\n");

    // When
    let result = search(&repo);

    // Then
    assert_eq!(result[0]["flags"], json!(["source_changed"]));
    assert_eq!(result[0]["revision"], initial[0]["revision"]);
    assert_eq!(
        result[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["element_id", "flags", "lines", "path", "revision"]
    );
    // Given an explicit metadata edit; addressed bytes still differ.
    let changed = create_basis(repo.get_root(), &[("code.rs", b"other\n")]);
    repo.write(
        "note.md",
        build_note(Some(&changed), &["code.rs"], "needle").as_bytes(),
    );
    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!(["source_changed"]));
    // Given
    let current = create_basis(repo.get_root(), &[("code.rs", b"new\n")]);
    repo.write(
        "note.md",
        build_note(Some(&current), &["code.rs"], "needle").as_bytes(),
    );
    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!([]));
}

#[test]
fn missing_source_remains_searchable_and_check_reports_owner() {
    // Given
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("code.rs", b"old\n")]);
    repo.write(
        "note.md",
        build_note(Some(&basis), &["code.rs"], "needle").as_bytes(),
    );

    // When
    let result = search(&repo);
    let problems = check(&repo, b"{}");

    // Then
    assert_eq!(result[0]["element_id"], "note.md");
    assert_eq!(result[0]["flags"], json!(["source_not_found"]));
    assert_eq!(
        problems,
        json!([{"code":"validation_failed","message":"knowledge source is not found","details":{"path":"note.md","element_id":"note.md","reason":"source_not_found","target":"code.rs"}}])
    );
    assert_eq!(check(&repo, br#"{"paths":["other.md"]}"#), json!([]));
}

#[test]
fn missing_basis_is_diagnosed_without_dropping_knowledge() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("code.rs", b"current");
    repo.write(
        "note.md",
        build_note(None, &["code.rs"], "needle").as_bytes(),
    );

    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!(["source_changed"]));
    assert_eq!(
        check(&repo, b"{}"),
        json!([{"code":"validation_failed","message":"knowledge source snapshot is missing","details":{"path":"note.md","element_id":"note.md","reason":"missing_source_snapshot"}}])
    );
}

#[test]
fn unavailable_basis_and_missing_historical_range_keep_precise_reasons() {
    // Given
    let repo = ReadingRepo::new();
    let unknown = TreeOid::parse(&"a".repeat(40)).unwrap();
    repo.write("code.rs", b"first\nsecond\n");
    repo.write(
        "note.md",
        build_note(Some(&unknown), &["code.rs"], "needle").as_bytes(),
    );
    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!(["source_changed"]));
    assert_eq!(
        check(&repo, b"{}")[0]["details"]["reason"],
        "source_snapshot_unavailable"
    );
    // Given
    let basis = create_basis(repo.get_root(), &[("code.rs", b"first\n")]);
    repo.write(
        "note.md",
        build_note(Some(&basis), &["code.rs#L2-L2"], "needle").as_bytes(),
    );

    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!(["source_not_found"]));
    assert_eq!(check(&repo, b"{}")[0]["details"]["target"], "code.rs#L2-L2");
}

#[test]
fn stale_knowledge_merges_sorted_deduplicated_source_flags() {
    // Given
    let repo = ReadingRepo::new();
    let basis = create_basis(
        repo.get_root(),
        &[("code.rs", b"old\n"), ("gone.rs", b"old\n")],
    );
    repo.write("code.rs", b"old\n");
    repo.write("gone.rs", b"old\n");
    repo.write(
        "note.md",
        build_note(
            Some(&basis),
            &["code.rs", "code.rs#L1-L1", "gone.rs"],
            "needle",
        )
        .as_bytes(),
    );
    assert_eq!(search(&repo)[0]["flags"], json!([]));
    repo.write("note.md", b"<!-- letopis: broken -->\nneedle\n");
    repo.write("code.rs", b"new\n");
    fs::remove_file(repo.get_root().join("gone.rs")).unwrap();

    // When / Then
    assert_eq!(
        search(&repo)[0]["flags"],
        json!(["stale", "source_changed", "source_not_found"])
    );
}

#[test]
fn remote_unfetched_is_not_reported_as_missing() {
    // Given
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[]);
    repo.write(
        "note.md",
        build_note(
            Some(&basis),
            &["https://unreachable.invalid/a?secret=kept#frag"],
            "needle",
        )
        .as_bytes(),
    );

    // When / Then
    assert_eq!(search(&repo)[0]["flags"], json!([]));
    assert_eq!(check(&repo, b"{}"), json!([]));
}

fn search(repo: &ReadingRepo) -> Value {
    invoke(repo, "search", br#"{"query":"needle","limit":10}"#)["result"].clone()
}
fn check(repo: &ReadingRepo, input: &[u8]) -> Value {
    invoke(repo, "check", input)["result"]["errors"].clone()
}
fn invoke(repo: &ReadingRepo, command: &str, input: &[u8]) -> Value {
    let output = repo.invoke(&[command], input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
