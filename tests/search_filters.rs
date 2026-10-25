mod support;

use letopis::domain::TreeOid;
use serde_json::{Value, json};
use std::fs;
use support::{
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

#[test]
fn empty_and_duplicate_selectors_preserve_literal_ids() {
    let repo = ReadingRepo::new();
    repo.write("code.rs", b"old\n");
    let basis = create_basis(repo.get_root(), &[("code.rs", b"old\n")]);
    write_note(&repo, "a.md", Some(&basis), &["code.rs"]);
    write_note(&repo, "b.md", None, &[]);
    // Each note has one query token; only a.md has an extra visible source label.
    // BM25 length normalization therefore orders the shorter b.md first.
    for filters in [json!({}), json!({"sources":[],"source_revisions":[]})] {
        assert_eq!(search_ids(&repo, &filters), ["b.md", "a.md"]);
    }
    assert_eq!(
        ids(&invoke(&repo, &json!({"query":"needle","limit":10}))["result"]),
        ["b.md", "a.md"]
    );
    for filters in [
        json!({"sources":["code.rs"]}),
        json!({"sources":["./code.rs","code.rs"]}),
        json!({"sources":["unknown.rs","code.rs"]}),
        json!({"source_revisions":[basis.as_str(),basis.as_str()]}),
    ] {
        assert_eq!(search_ids(&repo, &filters), ["a.md"]);
    }
}

#[test]
fn sources_use_exact_normalized_addresses_and_opaque_urls() {
    let repo = ReadingRepo::new();
    write_note(&repo, "range.md", None, &["code.rs#L1-L1"]);
    write_note(
        &repo,
        "remote.md",
        None,
        &["https://example.invalid/a?q=one#frag"],
    );
    for (source, expected) in [
        ("./src/../code.rs#L1-L1", vec!["range.md"]),
        ("code.rs", vec![]),
        ("https://example.invalid/a?q=one#frag", vec!["remote.md"]),
        ("https://example.invalid/a?q=two#frag", vec![]),
        ("https://example.invalid/a?q=one#other", vec![]),
        ("unknown.rs", vec![]),
    ] {
        assert_eq!(
            search_ids(&repo, &json!({"sources":[source]})),
            expected,
            "{source}"
        );
    }
}

#[test]
fn source_and_revision_require_one_binding_and_fields_combine_with_and() {
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("a.rs", b"a\n"), ("b.rs", b"b\n")]);
    let other = create_basis(repo.get_root(), &[("a.rs", b"changed\n"), ("b.rs", b"b\n")]);
    repo.write("a.rs", b"a\n");
    repo.write("b.rs", b"b\n");
    write_note(&repo, "both.md", Some(&basis), &["a.rs", "b.rs"]);
    write_note(&repo, "none.md", None, &[]);
    assert_eq!(
        search_ids(
            &repo,
            &json!({"sources":["a.rs"],"source_revisions":[other.as_str()]})
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        search_ids(
            &repo,
            &json!({"sources":["b.rs"],"source_revisions":[other.as_str()]})
        ),
        ["both.md"]
    );
    assert_eq!(
        search_ids(
            &repo,
            &json!({"sources":["a.rs"],"source_revisions":[other.as_str(),basis.as_str()]})
        ),
        ["both.md"]
    );
    assert_eq!(
        search_ids(&repo, &json!({"sources":["a.rs"],"needs_recheck":true})),
        Vec::<String>::new()
    );
    assert_eq!(
        search_ids(
            &repo,
            &json!({"sources":["missing.rs"],"source_revisions":[basis.as_str()]})
        ),
        Vec::<String>::new()
    );
}

#[test]
fn revision_equality_uses_per_note_basis_and_complete_selected_bytes() {
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("code.rs", b"first\nselected\n")]);
    let equal_range = create_basis(repo.get_root(), &[("code.rs", b"outside\nselected\n")]);
    let short = create_basis(repo.get_root(), &[("code.rs", b"first\n")]);
    let missing = create_basis(repo.get_root(), &[]);
    write_note(&repo, "a.md", Some(&basis), &["code.rs#L2-L2"]);
    write_note(&repo, "b.md", Some(&equal_range), &["code.rs"]);
    write_note(&repo, "c.md", None, &["code.rs"]);
    write_note(&repo, "f.md", Some(&short), &["code.rs#L2-L2"]);
    let unknown = TreeOid::parse(&"a".repeat(40)).unwrap();
    write_note(&repo, "d.md", Some(&unknown), &["code.rs"]);
    write_note(&repo, "e.md", Some(&basis), &["https://example.invalid/a"]);
    assert_ne!(basis, equal_range);
    // Both eligible notes render exactly "needle source", so their metrics tie by ID.
    assert_eq!(
        search_ids(&repo, &json!({"source_revisions":[equal_range.as_str()]})),
        ["a.md", "b.md"]
    );
    assert_eq!(
        search_ids(&repo, &json!({"source_revisions":[basis.as_str()]})),
        ["a.md"]
    );
    for tree in [short, missing] {
        assert_eq!(
            search_ids(&repo, &json!({"source_revisions":[tree.as_str()]})),
            Vec::<String>::new()
        );
    }
}

#[test]
fn current_changed_missing_and_invalid_notes_drive_all_recheck_modes() {
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("code.rs", b"old\n")]);
    repo.write("code.rs", b"old\n");
    write_note(&repo, "note.md", Some(&basis), &["code.rs"]);
    assert_eq!(
        search_ids(&repo, &json!({"needs_recheck":false})),
        ["note.md"]
    );
    assert_eq!(
        search_ids(&repo, &json!({"needs_recheck":true})),
        Vec::<String>::new()
    );
    for (bytes, flag) in [
        (Some(b"new\n".as_slice()), "source_changed"),
        (None, "source_not_found"),
    ] {
        if let Some(bytes) = bytes {
            repo.write("code.rs", bytes);
        } else {
            fs::remove_file(repo.get_root().join("code.rs")).unwrap();
        }
        let links = search(&repo, &json!({"needs_recheck":true}));
        assert_eq!(ids(&links), ["note.md"]);
        assert_eq!(links[0]["flags"], json!([flag]));
        assert_eq!(
            search_ids(&repo, &json!({"needs_recheck":false})),
            Vec::<String>::new()
        );
        assert_eq!(search_ids(&repo, &json!({})), ["note.md"]);
    }
    repo.write("code.rs", b"old\n");
    repo.write("note.md", b"<!-- letopis: broken -->\nneedle\n");
    let links = search(&repo, &json!({"needs_recheck":true}));
    assert_eq!(ids(&links), ["note.md"]);
    assert_eq!(links[0]["flags"], json!(["stale"]));
    assert_eq!(
        search_ids(&repo, &json!({"needs_recheck":false})),
        Vec::<String>::new()
    );
    assert_eq!(search_ids(&repo, &json!({})), ["note.md"]);
}

#[test]
fn invalid_filter_shapes_and_addresses_are_invalid_requests() {
    let repo = ReadingRepo::new();
    for filters in [
        json!(null),
        json!([]),
        json!({"sources":null}),
        json!({"sources":"a"}),
        json!({"sources":[null]}),
        json!({"sources":["../outside"]}),
        json!({"sources":["code.rs#L2-L1"]}),
        json!({"needs_recheck":null}),
        json!({"needs_recheck":"true"}),
        json!({"extra":1}),
        json!({"source_revisions":null}),
        json!({"source_revisions":["main"]}),
    ] {
        let value = invoke(
            &repo,
            &json!({"query":"needle","limit":1,"filters":filters}),
        );
        assert_eq!(value["error"]["code"], "invalid_request", "{filters}");
    }
}

#[test]
fn every_requested_root_is_validated_even_after_a_matching_tree() {
    let repo = ReadingRepo::new();
    let valid = create_basis(repo.get_root(), &[("code.rs", b"old\n")]);
    let missing = "f".repeat(40);
    assert!(valid.as_str() < missing.as_str());
    let mut command = std::process::Command::new("git");
    support::git::isolate_command(&mut command);
    assert!(
        !command
            .current_dir(repo.get_root())
            .args(["cat-file", "-e", &missing])
            .output()
            .unwrap()
            .status
            .success()
    );
    write_note(&repo, "note.md", Some(&valid), &["code.rs"]);
    for (query, sources) in [
        ("", json!([])),
        (" * : ", json!([])),
        ("absent", json!([])),
        ("needle", json!(["missing.rs"])),
        ("needle", json!(["code.rs"])),
    ] {
        let value = invoke(
            &repo,
            &json!({"query":query,"limit":1,"filters":{
            "sources":sources,"source_revisions":[valid.as_str(),missing]}}),
        );
        assert_eq!(value["error"]["code"], "not_found", "{query:?}");
    }
    let empty = ReadingRepo::new();
    let value = invoke(
        &empty,
        &json!({"query":"needle","limit":1,"filters":{"source_revisions":[missing]}}),
    );
    assert_eq!(value["error"]["code"], "not_found");
}

#[test]
fn ineligible_fts_head_cannot_consume_the_final_limit() {
    let repo = ReadingRepo::new();
    write_note(&repo, "a.md", None, &["code.rs"]);
    write_note(&repo, "b.md", None, &[]);
    let value = invoke(
        &repo,
        &json!({"query":"needle","limit":1,"filters":{"sources":["code.rs"]}}),
    );
    assert_eq!(value["ok"], true);
    assert_eq!(ids(&value["result"]), ["a.md"]);
}

fn write_note(repo: &ReadingRepo, path: &str, basis: Option<&TreeOid>, sources: &[&str]) {
    repo.write(path, build_note(basis, sources, "needle").as_bytes());
}
fn search_ids(repo: &ReadingRepo, filters: &Value) -> Vec<String> {
    ids(&search(repo, filters))
}
fn search(repo: &ReadingRepo, filters: &Value) -> Value {
    let value = invoke(
        repo,
        &json!({"query":"needle","limit":10,"filters":filters}),
    );
    assert_eq!(value["ok"], true, "{value}");
    value["result"].clone()
}
fn invoke(repo: &ReadingRepo, body: &Value) -> Value {
    let output = repo.invoke(&["search"], body.to_string().as_bytes());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn ids(links: &Value) -> Vec<String> {
    links
        .as_array()
        .unwrap()
        .iter()
        .map(|link| link["element_id"].as_str().unwrap().to_owned())
        .collect()
}
