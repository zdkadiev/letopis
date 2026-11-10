mod support;

use letopis::domain::TreeOid;
use serde_json::{Value, json};
use std::{fs, process::Output};
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

#[test]
fn shared_ranges_keep_public_results_and_author_basis_across_live_edits() {
    // Given: two owners sharing overlapping ranges of one author file.
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("code.rs", b"one\ntwo\nthree\n")]);
    repo.write("code.rs", b"one\ntwo\nthree\n");
    let a = build_note(
        Some(&basis),
        &["code.rs#L1-L1", "code.rs#L1-L2"],
        "needle [peer](b.md \"letopis:type=supports;path=b.md\")",
    );
    let b = build_note(Some(&basis), &["code.rs#L2-L2", "code.rs#L2-L3"], "needle");
    repo.write("a.md", a.as_bytes());
    repo.write("b.md", b.as_bytes());
    let head = fs::read(repo.get_root().join(".git/HEAD")).unwrap();
    let index = fs::read(repo.get_root().join(".git/index")).unwrap();
    let head_ref_path = std::str::from_utf8(&head)
        .unwrap()
        .trim()
        .strip_prefix("ref: ")
        .unwrap();
    let head_ref = fs::read(repo.get_root().join(".git").join(head_ref_path)).unwrap();
    // When / Then: clean commands agree and full indexing advances the revision.
    assert_eq!(check(&repo, b"{}"), json!([]));
    let initial_revision = invoke(&repo, "status", b"{}")["result"]["revision"].clone();
    assert_shared_range_results(&repo, &json!([]), &json!([]), &initial_revision);
    assert_eq!(
        invoke(&repo, "status", b"{}")["result"]["problems"],
        json!([])
    );
    assert_success(&repo.invoke(&["index", "--full"], b"{}"));
    let revision = invoke(&repo, "status", b"{}")["result"]["revision"].clone();
    assert_eq!(revision, initial_revision.as_u64().unwrap() + 1);
    assert_shared_range_results(&repo, &json!([]), &json!([]), &revision);
    // When / Then: live edits change observations without author metadata edits.
    verify_live_range_changes(&repo, &revision);
    // Then: commands preserve canonical knowledge, HEAD identity and real index.
    assert_eq!(
        fs::read(repo.get_root().join(".git").join(head_ref_path)).unwrap(),
        head_ref
    );
    for (path, bytes) in [
        ("a.md", a.as_bytes()),
        ("b.md", b.as_bytes()),
        (".git/HEAD", head.as_slice()),
        (".git/index", index.as_slice()),
    ] {
        assert_eq!(fs::read(repo.get_root().join(path)).unwrap(), bytes);
    }
}

fn verify_live_range_changes(repo: &ReadingRepo, revision: &Value) {
    // When: line two changes and the final addressed line disappears.
    repo.write("code.rs", b"one\nchanged\n");
    let changed = json!(["source_changed"]);
    let missing = json!(["source_changed", "source_not_found"]);
    assert_shared_range_results(repo, &changed, &missing, revision);
    let errors = check(repo, b"{}");
    // Then: public diagnostics identify only the missing range and preserve revision.
    assert_eq!(
        errors,
        json!([{"code":"validation_failed","message":"knowledge source is not found",
        "details":{"path":"b.md","element_id":"b.md","reason":"source_not_found","target":"code.rs#L2-L3"}}])
    );
    let status = invoke(repo, "status", b"{}");
    assert_eq!(status["result"]["problems"], errors);
    assert_eq!(status["result"]["revision"], *revision);
    assert_success(&repo.invoke(&["index", "--full"], b"{}"));
    assert_shared_range_results(
        repo,
        &changed,
        &missing,
        &json!(revision.as_u64().unwrap() + 1),
    );
}

fn assert_shared_range_results(
    repo: &ReadingRepo,
    a_flags: &Value,
    b_flags: &Value,
    revision: &Value,
) {
    let found = search(repo);
    assert_eq!(found.as_array().unwrap().len(), 2);
    for (id, flags) in [("a.md", a_flags), ("b.md", b_flags)] {
        let link = found
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["element_id"] == id)
            .unwrap();
        assert_eq!(link["flags"], *flags);
        assert_eq!(link["revision"], *revision);
    }
    let walk = invoke(repo, "traverse", br#"{"element_ids":["a.md"],"mode":"relations","direction":"outgoing","depth":1,"limit":10,"edge_types":["supports"]}"#);
    assert_eq!(walk["result"]["elements"].as_array().unwrap().len(), 1);
    assert_eq!(walk["result"]["elements"][0]["element_id"], "b.md");
    assert_eq!(walk["result"]["elements"][0]["flags"], *b_flags);
    assert_eq!(walk["result"]["elements"][0]["revision"], *revision);
    let inspected = invoke(
        repo,
        "inspect",
        br#"{"element_ids":["a.md","b.md","code.rs#L1-L1","code.rs#L2-L3"]}"#,
    );
    assert_eq!(
        inspected["result"]["elements"]["code.rs#L1-L1"]["hash"],
        "2c8b08da5ce60398e1f19af0e5dccc744df274b826abe585eaba68c525434806"
    );
    if b_flags == &json!([]) {
        assert_eq!(
            inspected["result"]["elements"]["code.rs#L2-L3"]["hash"],
            "f3952ccd5acbc3122b2fdc39d122b73e55f403fcb49dc411de7da4b4e987c07f"
        );
    } else {
        assert_eq!(
            inspected["result"]["elements"]["code.rs#L2-L3"]["hash"],
            Value::Null
        );
    }
}

fn search(repo: &ReadingRepo) -> Value {
    invoke(repo, "search", br#"{"query":"needle","limit":10}"#)["result"].clone()
}

fn check(repo: &ReadingRepo, input: &[u8]) -> Value {
    invoke(repo, "check", input)["result"]["errors"].clone()
}

fn invoke(repo: &ReadingRepo, command: &str, input: &[u8]) -> Value {
    assert_success(&repo.invoke(&[command], input))
}

fn assert_success(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    response
}
