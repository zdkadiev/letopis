mod support;
use letopis::{domain::RepoPath, markdown};
use support::{build_whole_file, parse_file};

#[test]
fn golden_search_representations() {
    // Given: each fixture has independent literal search text and SHA-256 expectations.
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown/expected.json")).unwrap();
    for (name, rows) in expected.as_object().unwrap() {
        let bytes = std::fs::read(format!(
            "{}/tests/fixtures/markdown/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let path = RepoPath::parse(&format!("knowledge/{name}")).unwrap();
        // When: parse the fixture bytes.
        let got = markdown::file_parse(&path, &bytes).unwrap().unwrap();
        // Then: rendered search text and its fingerprint match the literals.
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

#[test]
fn metadata_and_link_changes_keep_search_hash() {
    // Given
    let oid = "a".repeat(40);
    let other = "b".repeat(40);
    // When: establish the baseline projection.
    let base = parse_file(
        "a.md",
        &build_note("wal", 0, &oid, "knowledge/db.md", "depends_on"),
    );
    // Then
    assert_eq!(base.elements[0].search_text, "WAL\nUse safe SQLite.");
    // Given: change one metadata/link field per case.
    for changed in [
        build_note("wal", 1, &oid, "knowledge/db.md", "depends_on"),
        build_note("wal", 0, &other, "knowledge/db.md", "depends_on"),
        build_note("wal", 0, &oid, "knowledge/other.md", "depends_on"),
        build_note("wal", 0, &oid, "knowledge/db.md", "includes"),
        build_note("new", 0, &oid, "knowledge/db.md", "depends_on"),
    ] {
        // When
        let next = parse_file("a.md", &changed);
        // Then
        assert_ne!(base.elements[0].element.hash, next.elements[0].element.hash);
        assert_eq!(base.elements[0].search_text, next.elements[0].search_text);
        assert_eq!(base.elements[0].search_hash, next.elements[0].search_hash);
    }
}

#[test]
fn own_text_label_and_code_change_search_hash() {
    // Given: independently varying visible prose, label and code.
    let original = build_whole_file(
        "Own text [Label](b.md \"letopis:type=depends_on;path=b.md\").\n\n```rs\nlet a = 1;\n```\n",
    );
    let base = parse_file("a.md", &original);
    // Fixture baseline supplies the expected search hash.
    for changed in [
        original.replace("Own text", "New text"),
        original.replace("Label", "Other label"),
        original.replace("a = 1", "a = 2"),
    ] {
        // When: parse each visible-content edit.
        let next = parse_file("a.md", &changed);
        // Then: changed visible bytes must alter the search hash.
        assert_ne!(base.elements[0].search_hash, next.elements[0].search_hash);
    }
}

#[test]
fn moving_path_preserves_text_hashes() {
    // Given
    let text = build_whole_file("Stable text.");
    // When
    let a = parse_file("a.md", &text);
    let b = parse_file("b.md", &text);
    // Then
    assert_ne!(a.elements[0].element.id, b.elements[0].element.id);
    assert_eq!(a.elements[0].element.hash, b.elements[0].element.hash);
    assert_eq!(a.elements[0].search_hash, b.elements[0].search_hash);
    assert_eq!(a.elements[0].search_text, "Stable text.");
}

#[test]
fn code_examples_remain_searchable() {
    // Given
    let source = std::str::from_utf8(include_bytes!("fixtures/markdown/code.md")).unwrap();
    // When
    let got = parse_file("a.md", source);
    // Then
    let text = &got.elements[0].search_text;
    assert!(text.contains("<!-- letopis: {\"created_at\":8} -->"));
    assert!(text.contains("## Fake {#fake}"));
    assert!(text.contains("letopis:type=includes"));
    assert!(!text.contains("created_at\":3"));
}

#[test]
fn reference_labels_use_whole_file_context() {
    // Given
    let source = std::str::from_utf8(include_bytes!("fixtures/markdown/references.md")).unwrap();
    // When
    let got = parse_file("a.md", source);
    // Then
    assert_eq!(got.elements[0].search_text, "Note\nShown");
    assert!(got.edges.is_empty());
}

#[test]
fn search_text_preserves_word_and_block_boundaries() {
    // Given
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
        let text = build_whole_file(body);
        // When
        let got = parse_file("a.md", &text);
        // Then
        assert_eq!(got.elements[0].search_text, want, "{body}");
    }
}

#[test]
fn crlf_metadata_line_does_not_add_search_whitespace() {
    // Given
    let lf = "## A {#a}\n<!-- letopis: {\"created_at\":7} -->\nHi\n";
    let crlf = lf.replace('\n', "\r\n");
    // When
    let a = parse_file("a.md", lf);
    let b = parse_file("a.md", &crlf);
    // Then
    assert_eq!(b.elements[0].search_text, "A\nHi");
    assert_eq!(a.elements[0].search_hash, b.elements[0].search_hash);
    assert_ne!(a.elements[0].element.hash, b.elements[0].element.hash);
}

#[test]
fn equivalent_link_syntax_preserves_literal_search_text_and_hash() {
    // Given: canonical and navigation spellings may change without visible text.
    let original = build_whole_file("Static [Label](b.md \"letopis:type=related_to;path=b.md\").");
    let base = parse_file("a.md", &original);
    for changed in [
        original.replace("(b.md ", "(./b.md#viewer "),
        original.replace("(b.md ", "(%62.md#different "),
        original.replace("path=b.md", "path=./b.md"),
    ] {
        // When: parse equivalent local link spellings.
        let next = parse_file("a.md", &changed);
        // Then: exact bytes change while the independently hashed visible text stays fixed.
        assert_ne!(base.elements[0].element.hash, next.elements[0].element.hash);
        assert_eq!(next.elements[0].search_text, "Static Label.");
        assert_eq!(
            next.elements[0].search_hash.to_hex(),
            "81355fad83b8908a4d1b457f6110f23d6adfb887d5ec9cbd7514e683a932e01d"
        );
        assert_eq!(base.elements[0].search_hash, next.elements[0].search_hash);
    }
}

fn build_note(anchor: &str, created: u64, snapshot: &str, target: &str, kind: &str) -> String {
    format!(
        "## WAL {{#{anchor}}}\n<!-- letopis: {{\"created_at\":{created},\"source_snapshot\":\"{snapshot}\"}} -->\nUse **safe** [SQLite]({target} \"letopis:type={kind};path={target}\").\n"
    )
}
