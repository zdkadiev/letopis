mod support;
use letopis::{
    domain::RepoPath,
    markdown::{self, DiagnosticKind, ParsedElement},
};
use support::{assert_error, build_whole_file, parse_file};

#[test]
fn golden_elements_have_exact_bytes_and_metadata() {
    // Given: literal golden metadata and spans accompany each fixture.
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown/expected.json")).unwrap();
    for (name, expected) in golden.as_object().unwrap() {
        let bytes = std::fs::read(format!(
            "{}/tests/fixtures/markdown/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let path = RepoPath::parse(&format!("knowledge/{name}")).unwrap();
        // When: parse the exact fixture bytes.
        let result = markdown::file_parse(&path, &bytes).unwrap().unwrap();
        // Then: every projected element matches the independent literals.
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
    // Given: CRLF and missing-final-newline fixtures retain exact byte boundaries.
    let path = RepoPath::parse("a.md").unwrap();
    for bytes in [
        include_bytes!("fixtures/markdown/crlf.md").as_slice(),
        include_bytes!("fixtures/markdown/no-final-newline.md").as_slice(),
    ] {
        // When: parse each raw fixture.
        let got = markdown::file_parse(&path, bytes).unwrap().unwrap();
        // Then: the element ends at the exact byte and line offsets.
        assert_eq!(got.elements[0].element.bytes.get_end(), bytes.len());
        assert_eq!(got.elements[0].element.lines.get_end(), 3);
    }
    // Given: a Unicode section starts after an unmarked prefix.
    let text = std::str::from_utf8(include_bytes!("fixtures/markdown/unicode.md")).unwrap();
    // When: parse the section with its original UTF-8 bytes.
    let got = parse_file("a.md", text);
    // Then: the independently counted byte and line starts are preserved.
    assert_eq!(got.elements[0].element.bytes.get_start(), 11);
    assert_eq!(got.elements[0].element.lines.get_start(), 3);
}

#[test]
fn unmarked_markdown_is_not_corpus_and_invalid_utf8_is_rejected() {
    // Given: one path receives either plain Markdown or invalid UTF-8.
    let path = RepoPath::parse("a.md").unwrap();
    // When: parse both byte sequences.
    let plain = markdown::file_parse(&path, b"# plain\nText.\n");
    let invalid_utf8 = markdown::file_parse(&path, b"\xff");
    // Then: plain content has no projection and invalid bytes have a typed diagnostic.
    assert!(plain.unwrap().is_none());
    assert_error(invalid_utf8, DiagnosticKind::InvalidUtf8);
}

#[test]
fn code_examples_do_not_declare_elements() {
    // Given: code examples contain text resembling element declarations.
    let path = RepoPath::parse("a.md").unwrap();
    for text in [
        "```md\n## A {#a}\n<!-- letopis: {} -->\n```\n",
        "    ## A {#a}\n    <!-- letopis: {} -->\n",
        "`<!-- letopis: {} -->`\n",
        "```md\n## A {#a}\n<!-- letopis: {} -->\n",
    ] {
        // When: parse each code-only document.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: none of the examples becomes corpus data.
        assert!(result.unwrap().is_none(), "{text}");
    }
    // Given: a marked fixture contains fenced declaration examples.
    let text = std::str::from_utf8(include_bytes!("fixtures/markdown/code.md")).unwrap();
    // When: parse the real element and its examples together.
    let got = parse_file("a.md", text);
    // Then: only the real element is projected and no example creates an edge.
    assert_eq!(got.elements.len(), 1);
    assert!(got.edges.is_empty());
}

#[test]
fn file_errors_never_return_partial_elements() {
    // Given: a valid element precedes an invalid declaration.
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
        // When: parse all declarations in the file.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: reject the whole file without a partial projection.
        assert_error(result, DiagnosticKind::InvalidMetadata);
    }
    // Given: a valid element precedes an invalid declaration.
    let text = format!(
        "{prefix}## A {{#a}}\n<!-- letopis: {{\"created_at\":2}} -->\n[self](./a.md#a \"letopis:type=depends_on;path=knowledge/a.md#a\")\n"
    );
    // When: parse all declarations in the file.
    let result = markdown::file_parse(&path, text.as_bytes());
    // Then: reject the whole file without a partial projection.
    assert_error(result, DiagnosticKind::SelfLoop);
}

#[test]
fn nested_elements_and_orphan_metadata_are_rejected() {
    // Given: each document contains nested elements or metadata without an owning heading.
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
        // When: parse the complete invalid document.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: the independently specified layout diagnostic rejects the file.
        assert_error(result, kind);
    }
}

#[test]
fn anchors_are_unique_across_real_headings() {
    // Given: two actual headings repeat the same anchor.
    let path = RepoPath::parse("knowledge/a.md").unwrap();
    let duplicate = b"## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n## B {#a}\n";
    // When: parse the duplicate headings.
    let result = markdown::file_parse(&path, duplicate);
    // Then: the duplicate anchor is rejected.
    assert_error(result, DiagnosticKind::DuplicateAnchor);

    // Given: each malformed anchor violates the declared anchor grammar.
    for anchor in ["L1-L2", "1bad", ""] {
        let text = format!("## A {{#{anchor}}}\n<!-- letopis: {{\"created_at\":1}} -->\n");
        // When: parse the marked section.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: the file reports the invalid-anchor diagnostic.
        assert_error(result, DiagnosticKind::InvalidAnchor);
    }
}

#[test]
fn snapshot_metadata_preserves_full_oid_and_zero_created_at() {
    // Given: independent expected lengths cover both Git wire formats.
    for n in [40, 64] {
        let oid = "a".repeat(n);
        let text =
            format!("<!-- letopis: {{\"created_at\":0,\"source_snapshot\":\"{oid}\"}} -->\nX");
        // When
        let got = parse_file("a.markdown", &text);
        // Then
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
    // Given: metadata is multiline or shares its line with other content.
    let path = RepoPath::parse("a.md").unwrap();
    for text in [
        "<!-- letopis: {\n\"created_at\":1} -->\n",
        "<!-- letopis: {\"created_at\":1} --> trailing\n",
        "<!-- letopis: {\"created_at\":1} --> <!-- letopis: {\"created_at\":2} -->\n",
    ] {
        // When: parse each malformed comment layout.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: every malformed layout rejects the complete file.
        assert!(result.is_err(), "{text}");
    }
}

#[test]
fn raw_html_content_does_not_declare_metadata() {
    // Given: raw HTML examples contain apparent metadata alongside one real marked element.
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
        // When: parse marked and unmarked versions of each HTML example.
        let got = parse_file("a.md", &marked);
        let unmarked = markdown::file_parse(&path, raw.as_bytes());
        // Then: only the real element is projected while raw content remains searchable.
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
    // Given: coordinates must retain a multiline comment and omit ordinary prose.
    let path = RepoPath::parse("a.md").unwrap();
    for (prefix, comment) in [
        ("intro ", "<!-- letopis: {\"created_at\":1} -->"),
        ("", "<!-- letopis: {\n\"created_at\":1} -->"),
    ] {
        let text = format!("{prefix}{comment}\n");
        // When: parse the invalid standalone or prefixed comment.
        let diagnostics = markdown::file_parse(&path, text.as_bytes()).unwrap_err();
        // Then: the diagnostic span highlights only the complete comment.
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

#[test]
fn adjacent_sections_preserve_lines_across_gaps_unicode_and_line_endings() {
    // Given: uneven sections include CRLF, a fenced fake heading, a gap and no final LF.
    let text = "前文\r\n\r\n## A {#a}\r\n<!-- letopis: {\"created_at\":1} -->\r\nА\r\n\r\n```md\r\n## Fake {#fake}\r\n```\r\n# Outside\r\nGap\r\n\r\n## B {#b}\r\n<!-- letopis: {\"created_at\":2} -->\r\nБ";
    // When: parse both non-overlapping elements with their complete document context.
    let parsed = parse_file("a.md", text);
    // Then: both section line ranges retain independent literal coordinates.
    assert_eq!(parsed.elements.len(), 2);
    assert_eq!(parsed.elements[0].element.lines.get_start(), 3);
    assert_eq!(parsed.elements[0].element.lines.get_end(), 9);
    assert_eq!(parsed.elements[1].element.lines.get_start(), 13);
    assert_eq!(parsed.elements[1].element.lines.get_end(), 15);
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
