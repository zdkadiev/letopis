mod support;
use letopis::{
    domain::RepoPath,
    markdown::{DiagnosticKind, ParsedElement, parse},
};
use support::{assert_error, build_whole_file, parse_file};

#[test]
fn golden_elements_have_exact_bytes_and_metadata() {
    // Arrange
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown/expected.json")).unwrap();
    for (name, expected) in golden.as_object().unwrap() {
        let bytes = std::fs::read(format!(
            "{}/tests/fixtures/markdown/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let path = RepoPath::parse(&format!("knowledge/{name}")).unwrap();
        // Act
        let result = parse(&path, &bytes).unwrap().unwrap();
        // Assert
        assert_eq!(
            result.elements.len(),
            expected.as_array().unwrap().len(),
            "{name}"
        );
        for (got, want) in result.elements.iter().zip(expected.as_array().unwrap()) {
            assert_golden_element(got, want, name);
        }
    }
}

#[test]
fn unicode_crlf_and_eof_have_exact_spans() {
    // Arrange
    let path = RepoPath::parse("a.md").unwrap();
    for bytes in [
        include_bytes!("fixtures/markdown/crlf.md").as_slice(),
        include_bytes!("fixtures/markdown/no-final-newline.md").as_slice(),
    ] {
        // Act
        let got = parse(&path, bytes).unwrap().unwrap();
        // Assert
        assert_eq!(got.elements[0].element.bytes.get_end(), bytes.len());
        assert_eq!(got.elements[0].element.lines.get_end(), 3);
    }
    // Arrange
    let text = std::str::from_utf8(include_bytes!("fixtures/markdown/unicode.md")).unwrap();
    // Act
    let got = parse_file("a.md", text);
    // Assert
    assert_eq!(got.elements[0].element.bytes.get_start(), 11);
    assert_eq!(got.elements[0].element.lines.get_start(), 3);
}

#[test]
fn unmarked_markdown_is_not_corpus_and_invalid_utf8_is_rejected() {
    // Arrange
    let path = RepoPath::parse("a.md").unwrap();
    // Act
    let plain = parse(&path, b"# plain\nText.\n");
    let invalid_utf8 = parse(&path, b"\xff");
    // Assert
    assert!(plain.unwrap().is_none());
    assert_error(invalid_utf8, DiagnosticKind::InvalidUtf8);
}

#[test]
fn code_examples_do_not_declare_elements() {
    // Arrange
    let path = RepoPath::parse("a.md").unwrap();
    for text in [
        "```md\n## A {#a}\n<!-- letopis: {} -->\n```\n",
        "    ## A {#a}\n    <!-- letopis: {} -->\n",
        "`<!-- letopis: {} -->`\n",
        "```md\n## A {#a}\n<!-- letopis: {} -->\n",
    ] {
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert!(result.unwrap().is_none(), "{text}");
    }
    // Arrange
    let text = std::str::from_utf8(include_bytes!("fixtures/markdown/code.md")).unwrap();
    // Act
    let got = parse_file("a.md", text);
    // Assert
    assert_eq!(got.elements.len(), 1);
    assert!(got.edges.is_empty());
}

#[test]
fn file_errors_never_return_partial_elements() {
    // Arrange
    let path = RepoPath::parse("knowledge/a.md").unwrap();
    let prefix = "## Valid {#valid}\n<!-- letopis: {\"created_at\":1} -->\nOK.\n\n";
    for json in [
        "{}",
        "{",
        "{\"created_at\":-1}",
        "{\"created_at\":1.5}",
        "{\"created_at\":18446744073709551616}",
        "{\"created_at\":0,\"kind\":\"vertex\"}",
        "{\"created_at\":0,\"source_snapshot\":null}",
        "{\"created_at\":0,\"source_snapshot\":\"BAD\"}",
        "{\"created_at\":0,\"created_at\":1}",
    ] {
        let text = format!("{prefix}## Bad {{#bad}}\n<!-- letopis: {json} -->\nOops.\n");
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert_error(result, DiagnosticKind::InvalidMetadata);
    }
    // Arrange
    let text = format!(
        "{prefix}## A {{#a}}\n<!-- letopis: {{\"created_at\":2}} -->\n[self](./knowledge/../knowledge/a.md#a \"letopis:type=depends_on\")\n"
    );
    // Act
    let result = parse(&path, text.as_bytes());
    // Assert
    assert_error(result, DiagnosticKind::SelfLoop);
}

#[test]
fn nested_elements_and_orphan_metadata_are_rejected() {
    // Arrange
    let path = RepoPath::parse("knowledge/a.md").unwrap();
    for (text, kind) in [
        (
            "## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n### B {#b}\n<!-- letopis: {\"created_at\":2} -->\n",
            DiagnosticKind::NestedElement,
        ),
        (
            "<!-- letopis: {\"created_at\":1} -->\n## B {#b}\n<!-- letopis: {\"created_at\":2} -->\n",
            DiagnosticKind::NestedElement,
        ),
        (
            "intro\n<!-- letopis: {\"created_at\":1} -->\n",
            DiagnosticKind::OrphanMetadata,
        ),
        (
            "## A {#a}\n\n<!-- letopis: {\"created_at\":1} -->\n",
            DiagnosticKind::OrphanMetadata,
        ),
        (
            "## A\n<!-- letopis: {\"created_at\":1} -->\n",
            DiagnosticKind::OrphanMetadata,
        ),
    ] {
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert_error(result, kind);
    }
}

#[test]
fn anchors_are_unique_across_real_headings() {
    // Arrange
    let path = RepoPath::parse("knowledge/a.md").unwrap();
    let duplicate = b"## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n## B {#a}\n";
    // Act
    let result = parse(&path, duplicate);
    // Assert
    assert_error(result, DiagnosticKind::DuplicateAnchor);

    // Arrange
    for anchor in ["L1-L2", "1bad", ""] {
        let text = format!("## A {{#{anchor}}}\n<!-- letopis: {{\"created_at\":1}} -->\n");
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert_error(result, DiagnosticKind::InvalidAnchor);
    }
}

#[test]
fn snapshot_metadata_preserves_full_oid_and_zero_created_at() {
    // Arrange: independent expected lengths cover both Git wire formats.
    for n in [40, 64] {
        let oid = "a".repeat(n);
        let text =
            format!("<!-- letopis: {{\"created_at\":0,\"source_snapshot\":\"{oid}\"}} -->\nX");
        // Act
        let got = parse_file("a.markdown", &text);
        // Assert
        assert_eq!(got.elements[0].element.metadata.created_at, 0);
        assert_eq!(
            got.elements[0]
                .element
                .metadata
                .source_snapshot
                .as_ref()
                .unwrap()
                .as_str(),
            oid
        );
    }
}

#[test]
fn metadata_must_fit_exactly_one_standalone_line() {
    // Arrange
    let path = RepoPath::parse("a.md").unwrap();
    for text in [
        "<!-- letopis: {\n\"created_at\":1} -->\n",
        "<!-- letopis: {\"created_at\":1} --> trailing\n",
        "<!-- letopis: {\"created_at\":1} --> <!-- letopis: {\"created_at\":2} -->\n",
    ] {
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert!(result.is_err(), "{text}");
    }
}

#[test]
fn raw_html_content_does_not_declare_metadata() {
    // Arrange
    let path = RepoPath::parse("a.md").unwrap();
    for (open, close) in [
        ("<script>", "</script>"),
        ("<style>", "</style>"),
        ("<textarea>", "</textarea>"),
        ("<pre>", "</pre>"),
        ("<!-- example", "-->"),
    ] {
        let raw = format!(
            "{open}\nconst example = `\n<!-- letopis: {{\"created_at\":2}} -->\n`;\n{close}\n"
        );
        let marked = build_whole_file(&format!("\n{raw}"));
        // Act
        let got = parse_file("a.md", &marked);
        let unmarked = parse(&path, raw.as_bytes());
        // Assert
        assert_eq!(got.elements.len(), 1, "{open}");
        assert!(
            got.elements[0].search_text.contains("created_at\":2"),
            "{open}"
        );
        assert!(unmarked.unwrap().is_none(), "{open}");
    }
}

#[test]
fn locates_the_complete_invalid_comment_without_unrelated_prefix_text() {
    // Arrange: coordinates must retain a multiline comment and omit ordinary prose.
    let path = RepoPath::parse("a.md").unwrap();
    for (prefix, comment) in [
        ("intro ", "<!-- letopis: {\"created_at\":1} -->"),
        ("", "<!-- letopis: {\n\"created_at\":1} -->"),
    ] {
        let text = format!("{prefix}{comment}\n");
        // Act
        let diagnostics = parse(&path, text.as_bytes()).unwrap_err();
        // Assert
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == DiagnosticKind::InvalidMetadata)
            .expect("invalid service comment must be reported");
        let span = diagnostic
            .bytes
            .expect("comment location must be available");
        let highlighted = &text[span.get_start()..span.get_end()];
        assert_eq!(highlighted.trim_end_matches(['\r', '\n']), comment);
    }
}

fn assert_golden_element(parsed: &ParsedElement, want: &serde_json::Value, name: &str) {
    let got = &parsed.element;
    assert_eq!(got.id.as_str(), want["id"].as_str().unwrap(), "{name}");
    assert_eq!(
        got.bytes.get_start() as u64,
        want["start"].as_u64().unwrap(),
        "{name}"
    );
    assert_eq!(
        got.bytes.get_end() as u64,
        want["end"].as_u64().unwrap(),
        "{name}"
    );
    assert_eq!(
        got.lines.get_start(),
        want["line_start"].as_u64().unwrap(),
        "{name}"
    );
    assert_eq!(
        got.lines.get_end(),
        want["line_end"].as_u64().unwrap(),
        "{name}"
    );
    assert_eq!(got.hash.to_hex(), want["hash"].as_str().unwrap(), "{name}");
    assert_eq!(
        got.metadata.created_at,
        want["created_at"].as_u64().unwrap(),
        "{name}"
    );
}
