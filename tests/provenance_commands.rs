mod support;

use letopis::domain::TreeOid;
use serde_json::{Value, json};
use std::{fs, process::Output};
use support::{
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

#[test]
fn check_and_status_report_changed_elements_alongside_existing_problems() {
    // Given: a corpus with changed bytes, an absent source, two unusable bases and invalid markup.
    let repo = ReadingRepo::new();
    write_source_findings_fixture(&repo);

    // When: check publishes the corpus and status observes that publication.
    let checked = invoke(&repo, "check", b"{}");
    let status = invoke(&repo, "status", b"{}");

    // Then: both commands report one problem for the element with its two changed addresses.
    assert_eq!(
        checked["result"]["problems"][0],
        json!({"code":"validation_failed","message":"knowledge source changed","details":{
            "element_id":"changed.md","path":"changed.md",
            "targets":["a.txt#L1-L1","b.txt"],"reason":"source_changed"
        }})
    );
    assert_eq!(status["result"]["problems"], checked["result"]["problems"]);
    assert!(status["result"].get("source_changed").is_none());
    let reasons: Vec<_> = checked["result"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|problem| {
            (
                problem["details"]["path"].as_str().unwrap(),
                problem["details"]["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        reasons,
        [
            ("changed.md", "source_changed"),
            ("invalid.md", "duplicate_anchor"),
            ("missing-basis.md", "missing_source_snapshot"),
            ("missing-source.md", "source_not_found"),
            ("unknown-basis.md", "source_snapshot_unavailable"),
        ]
    );

    // Given: the changed element now declares a basis with the current source bytes.
    let basis = create_basis(
        repo.get_root(),
        &[("a.txt", b"new\nsecond\n"), ("b.txt", b"new\n")],
    );
    repo.write(
        "changed.md",
        build_note(
            Some(&basis),
            &["a.txt#L1-L1", "a.txt#L2-L2", "b.txt"],
            "needle",
        )
        .as_bytes(),
    );

    // When: check refreshes the repaired binding and status reads it.
    let repaired = invoke(&repo, "check", b"{}");
    let status = invoke(&repo, "status", b"{}");

    // Then: the repaired element disappears and both commands still report the four other findings.
    let problems = repaired["result"]["problems"].as_array().unwrap();
    assert_eq!(
        problems,
        &checked["result"]["problems"].as_array().unwrap()[1..]
    );
    assert_eq!(status["result"]["problems"], repaired["result"]["problems"]);
}

#[test]
fn check_paths_filter_both_kinds_of_finding_after_corpus_refresh() {
    // Given: a corpus with different findings in different files and a clean owner.
    let repo = ReadingRepo::new();
    write_source_findings_fixture(&repo);
    repo.write_knowledge("clean.md", "Clean");

    for (paths, expected_reasons) in [
        (
            br#"{"paths":["changed.md"]}"#.as_slice(),
            ["source_changed"].as_slice(),
        ),
        (br#"{"paths":["missing-source.md"]}"#, &["source_not_found"]),
        (br#"{"paths":["clean.md"]}"#, &[]),
    ] {
        // When: check filters the report to one requested owner.
        let value = invoke(&repo, "check", paths);

        // Then: the problem array contains the selected owner's findings.
        let reasons: Vec<_> = value["result"]["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|problem| problem["details"]["reason"].as_str().unwrap())
            .collect();
        assert_eq!(reasons, expected_reasons);
    }

    // When: status observes the cache published by the scoped checks.
    let status = invoke(&repo, "status", b"{}");

    // Then: the refresh included owners outside the selected path.
    assert_eq!(status["result"]["problems"].as_array().unwrap().len(), 5);
}

#[test]
fn check_and_status_limit_the_combined_problems_to_one_hundred() {
    // Given: one dangling link and one hundred owners with missing source snapshots.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "000.md",
        "[missing](gone.md \"letopis:type=supports;path=gone.md\")",
    );
    repo.write("source.txt", b"present");
    let note = build_note(None, &["source.txt"], "needle");
    for number in 1..=100 {
        repo.write(&format!("{number:03}.md"), note.as_bytes());
    }

    // When: check publishes the corpus and status reads the same findings.
    let checked = invoke(&repo, "check", b"{}");
    let status = invoke(&repo, "status", b"{}");

    // Then: both reports contain the first hundred problems across both kinds.
    for report in [&status, &checked] {
        let problems = report["result"]["problems"].as_array().unwrap();
        assert_eq!(problems.len(), 100);
        assert_eq!(problems[0]["details"]["path"], "000.md");
        assert_eq!(problems[0]["details"]["reason"], "dangling_target");
        assert_eq!(problems[99]["details"]["path"], "099.md");
        assert_eq!(problems[99]["details"]["reason"], "missing_source_snapshot");
    }

    // When: check selects the owner beyond the unscoped output limit.
    let selected = invoke(&repo, "check", br#"{"paths":["100.md"]}"#);

    // Then: filtering before the limit includes that owner's problem.
    let problems = selected["result"]["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0]["details"]["path"], "100.md");
}

#[test]
fn clean_corpus_reports_empty_findings() {
    // Given: a clean repository with a valid knowledge element.
    let repo = ReadingRepo::new();
    repo.write_knowledge("clean.md", "Clean");

    // When: check and status return their reports.
    let checked = invoke(&repo, "check", b"{}");
    let status = invoke(&repo, "status", b"{}");

    // Then: both commands exit zero, with an empty problem array.
    assert_eq!(checked, json!({"ok":true,"result":{"problems":[]}}));
    assert_eq!(status["result"]["problems"], json!([]));
    assert!(status["result"].get("source_changed").is_none());
}

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
    // Given: an existing source with no declared source snapshot.
    let repo = ReadingRepo::new();
    repo.write("code.rs", b"current");
    repo.write(
        "note.md",
        build_note(None, &["code.rs"], "needle").as_bytes(),
    );

    // When: check publishes the corpus, status reads it, and search selects recheck candidates.
    let problems = check(&repo, b"{}");
    let status = invoke(&repo, "status", b"{}");
    let results = search_needing_recheck(&repo);

    // Then: the element needs recheck and both reports identify the missing snapshot.
    let links = results.as_array().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["element_id"], "note.md");
    assert_eq!(links[0]["flags"], json!(["source_changed"]));
    assert_eq!(links[0]["revision"], status["result"]["revision"]);
    assert_eq!(
        problems,
        json!([{"code":"validation_failed","message":"knowledge source snapshot is missing","details":{"path":"note.md","element_id":"note.md","reason":"missing_source_snapshot"}}])
    );
    assert_eq!(status["result"]["problems"], problems);
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
    // When: check publishes the clean corpus and status reads it.
    let problems = check(&repo, b"{}");
    let status = invoke(&repo, "status", b"{}");
    let initial_revision = &status["result"]["revision"];

    // Then: both owners are clean and neither needs recheck.
    assert_eq!(problems, json!([]));
    assert_eq!(status["result"]["problems"], problems);
    assert_eq!(search_needing_recheck(&repo), json!([]));
    assert_shared_range_results(&repo, &json!([]), &json!([]), initial_revision);

    // When: full indexing republishes the corpus before the live source changes.
    assert_success(&repo.invoke(&["index", "--full"], b"{}"));
    let revision = invoke(&repo, "status", b"{}")["result"]["revision"].clone();
    assert_eq!(revision, initial_revision.as_u64().unwrap() + 1);
    assert_shared_range_results(&repo, &json!([]), &json!([]), &revision);
    verify_live_range_changes(&repo, &revision);

    // Then: commands preserve canonical knowledge, the Git head and the real index.
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

fn write_source_findings_fixture(repo: &ReadingRepo) {
    let basis = create_basis(
        repo.get_root(),
        &[
            ("a.txt", b"old\nsecond\n"),
            ("b.txt", b"old\n"),
            ("gone.txt", b"old\n"),
        ],
    );
    let unknown_basis = TreeOid::parse(&"a".repeat(40)).unwrap();
    repo.write("a.txt", b"new\nsecond\n");
    repo.write("b.txt", b"new\n");
    for (path, basis, sources) in [
        (
            "changed.md",
            Some(&basis),
            ["a.txt#L1-L1", "a.txt#L2-L2", "b.txt"].as_slice(),
        ),
        ("missing-source.md", Some(&basis), &["gone.txt"]),
        ("missing-basis.md", None, &["a.txt"]),
        ("unknown-basis.md", Some(&unknown_basis), &["a.txt"]),
    ] {
        repo.write(path, build_note(basis, sources, "needle").as_bytes());
    }
    repo.write(
        "invalid.md",
        b"# One {#one}\n<!-- letopis: {\"created_at\":1} -->\n# Again {#one}\n",
    );
}

fn verify_live_range_changes(repo: &ReadingRepo, revision: &Value) {
    // Given: line two changes and the final addressed line disappears.
    repo.write("code.rs", b"one\nchanged\n");

    // When: unscoped check, status and filtered search observe the live edit.
    let problems = check(repo, b"{}");
    let status = invoke(repo, "status", b"{}");
    let results = search_needing_recheck(repo);

    // Then: both owners need recheck at the published revision, including the traversed peer.
    let links = results.as_array().unwrap();
    let changed = json!(["source_changed"]);
    let missing = json!(["source_changed", "source_not_found"]);
    assert_eq!(links.len(), 2);
    for (id, flags) in [("a.md", &changed), ("b.md", &missing)] {
        let link = links.iter().find(|link| link["element_id"] == id).unwrap();
        assert_eq!(link["flags"], *flags);
        assert_eq!(link["revision"], *revision);
    }
    assert_shared_range_results(repo, &changed, &missing, revision);
    assert_eq!(
        problems,
        json!([
            {"code":"validation_failed","message":"knowledge source changed",
             "details":{"path":"a.md","element_id":"a.md","reason":"source_changed","targets":["code.rs#L1-L2"]}},
            {"code":"validation_failed","message":"knowledge source changed",
             "details":{"path":"b.md","element_id":"b.md","reason":"source_changed","targets":["code.rs#L2-L2"]}},
            {"code":"validation_failed","message":"knowledge source is not found",
             "details":{"path":"b.md","element_id":"b.md","reason":"source_not_found","target":"code.rs#L2-L3"}}
        ])
    );
    assert_eq!(status["result"]["problems"], problems);
    assert_eq!(status["result"]["revision"], *revision);

    // When: full indexing runs again with the same live source bytes.
    assert_success(&repo.invoke(&["index", "--full"], b"{}"));

    // Then: the observations persist in the next revision.
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

fn search_needing_recheck(repo: &ReadingRepo) -> Value {
    invoke(
        repo,
        "search",
        br#"{"query":"needle","limit":10,"filters":{"needs_recheck":true}}"#,
    )["result"]
        .clone()
}

fn check(repo: &ReadingRepo, input: &[u8]) -> Value {
    invoke(repo, "check", input)["result"]["problems"].clone()
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
