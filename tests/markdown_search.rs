mod support;
use letopis::{adapters::markdown::parse, domain::RepoPath};
use support::*;

#[test]
fn golden_search_representations() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown/expected.json")).unwrap();
    for (name, rows) in expected.as_object().unwrap() {
        let bytes = std::fs::read(format!(
            "{}/tests/fixtures/markdown/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let got = parse(
            &RepoPath::parse(&format!("knowledge/{name}")).unwrap(),
            &bytes,
        )
        .unwrap()
        .unwrap();
        for (element, want) in got.elements.iter().zip(rows.as_array().unwrap()) {
            assert_eq!(
                element.search_text,
                want["search_text"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                element.search_hash.to_hex(),
                want["search_hash"].as_str().unwrap(),
                "{name}"
            );
        }
    }
}

fn note(anchor: &str, created: u64, snapshot: &str, target: &str, kind: &str) -> String {
    format!(
        "## WAL {{#{anchor}}}\n<!-- letopis: {{\"created_at\":{created},\"source_snapshot\":\"{snapshot}\"}} -->\nUse **safe** [SQLite]({target} \"letopis:type={kind}\").\n"
    )
}
#[test]
fn metadata_and_link_changes_keep_search_hash() {
    let oid = "a".repeat(40);
    let other = "b".repeat(40);
    let base = parsed(
        "a.md",
        &note("wal", 0, &oid, "knowledge/db.md", "depends_on"),
    );
    assert_eq!(base.elements[0].search_text, "WAL\nUse safe SQLite.");
    for changed in [
        note("wal", 1, &oid, "knowledge/db.md", "depends_on"),
        note("wal", 0, &other, "knowledge/db.md", "depends_on"),
        note("wal", 0, &oid, "knowledge/other.md", "depends_on"),
        note("wal", 0, &oid, "knowledge/db.md", "includes"),
        note("new", 0, &oid, "knowledge/db.md", "depends_on"),
    ] {
        let next = parsed("a.md", &changed);
        assert_ne!(base.elements[0].element.hash, next.elements[0].element.hash);
        assert_eq!(base.elements[0].search_text, next.elements[0].search_text);
        assert_eq!(base.elements[0].search_hash, next.elements[0].search_hash);
    }
}

#[test]
fn own_text_label_and_code_change_search_hash() {
    let original =
        whole("Own text [Label](b.md \"letopis:type=depends_on\").\n\n```rs\nlet a = 1;\n```\n");
    let base = parsed("a.md", &original);
    for changed in [
        original.replace("Own text", "New text"),
        original.replace("Label", "Other label"),
        original.replace("a = 1", "a = 2"),
    ] {
        assert_ne!(
            base.elements[0].search_hash,
            parsed("a.md", &changed).elements[0].search_hash
        );
    }
}

#[test]
fn moving_path_preserves_text_hashes() {
    let text = whole("Stable text.");
    let a = parsed("a.md", &text);
    let b = parsed("b.md", &text);
    assert_ne!(a.elements[0].element.id, b.elements[0].element.id);
    assert_eq!(a.elements[0].element.hash, b.elements[0].element.hash);
    assert_eq!(a.elements[0].search_hash, b.elements[0].search_hash);
    assert_eq!(a.elements[0].search_text, "Stable text.");
}

#[test]
fn code_examples_remain_searchable() {
    let got = parsed(
        "a.md",
        std::str::from_utf8(include_bytes!("fixtures/markdown/code.md")).unwrap(),
    );
    let text = &got.elements[0].search_text;
    assert!(text.contains("<!-- letopis: {\"created_at\":8} -->"));
    assert!(text.contains("## Fake {#fake}"));
    assert!(text.contains("letopis:type=includes"));
    assert!(!text.contains("created_at\":3"));
}

#[test]
fn reference_labels_use_whole_file_context() {
    let got = parsed(
        "a.md",
        std::str::from_utf8(include_bytes!("fixtures/markdown/references.md")).unwrap(),
    );
    assert_eq!(got.elements[0].search_text, "Note\nShown");
    assert!(got.edges.is_empty());
}

#[test]
fn search_text_preserves_word_and_block_boundaries() {
    for (body, want) in [
        ("hel**lo**", "hello"),
        ("one\n\ntwo", "one\ntwo"),
        ("a\nb  \nc", "a\nb\nc"),
        ("Text `code` after.", "Text code after."),
        ("<b>safe</b>", "<b>safe</b>"),
        ("![alt text](img.png)", "alt text"),
        ("A &amp; B", "A & B"),
        ("- one\n- two", "one\ntwo"),
    ] {
        assert_eq!(
            parsed("a.md", &whole(body)).elements[0].search_text,
            want,
            "{body}"
        );
    }
}

#[test]
fn crlf_metadata_line_does_not_add_search_whitespace() {
    let lf = "## A {#a}\n<!-- letopis: {\"created_at\":7} -->\nHi\n";
    let a = parsed("a.md", lf);
    let b = parsed("a.md", &lf.replace('\n', "\r\n"));
    assert_eq!(b.elements[0].search_text, "A\nHi");
    assert_eq!(a.elements[0].search_hash, b.elements[0].search_hash);
    assert_ne!(a.elements[0].element.hash, b.elements[0].element.hash);
}
