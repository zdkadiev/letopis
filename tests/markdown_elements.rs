mod support;
use letopis::{
    adapters::markdown::{DiagnosticKind::*, parse},
    domain::*,
};
use support::*;

#[test]
fn golden_elements_have_exact_bytes_and_metadata() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown/expected.json")).unwrap();
    for (name, expected) in golden.as_object().unwrap() {
        let bytes = std::fs::read(format!(
            "{}/tests/fixtures/markdown/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let result = parse(
            &RepoPath::parse(&format!("knowledge/{name}")).unwrap(),
            &bytes,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result.elements.len(),
            expected.as_array().unwrap().len(),
            "{name}"
        );
        for (parsed_element, want) in result.elements.iter().zip(expected.as_array().unwrap()) {
            let got = &parsed_element.element;
            assert_eq!(got.id.as_str(), want["id"].as_str().unwrap(), "{name}");
            assert_eq!(
                got.bytes.start() as u64,
                want["start"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                got.bytes.end() as u64,
                want["end"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                got.lines.start(),
                want["line_start"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                got.lines.end(),
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
    }
}

#[test]
fn unicode_crlf_and_eof_have_exact_spans() {
    for bytes in [
        include_bytes!("fixtures/markdown/crlf.md").as_slice(),
        include_bytes!("fixtures/markdown/no-final-newline.md").as_slice(),
    ] {
        let got = parse(&RepoPath::parse("a.md").unwrap(), bytes)
            .unwrap()
            .unwrap();
        assert_eq!(got.elements[0].element.bytes.end(), bytes.len());
        assert_eq!(got.elements[0].element.lines.end(), 3);
    }
    let got = parsed(
        "a.md",
        std::str::from_utf8(include_bytes!("fixtures/markdown/unicode.md")).unwrap(),
    );
    assert_eq!(got.elements[0].element.bytes.start(), 11);
    assert_eq!(got.elements[0].element.lines.start(), 3);
}

#[test]
fn unmarked_markdown_is_not_corpus_and_invalid_utf8_is_rejected() {
    assert!(
        parse(&RepoPath::parse("a.md").unwrap(), b"# plain\nText.\n")
            .unwrap()
            .is_none()
    );
    error(b"\xff", InvalidUtf8);
}

#[test]
fn code_examples_do_not_declare_elements() {
    for s in [
        "```md\n## A {#a}\n<!-- letopis: {} -->\n```\n",
        "    ## A {#a}\n    <!-- letopis: {} -->\n",
        "`<!-- letopis: {} -->`\n",
        "```md\n## A {#a}\n<!-- letopis: {} -->\n",
    ] {
        assert!(
            parse(&RepoPath::parse("a.md").unwrap(), s.as_bytes())
                .unwrap()
                .is_none(),
            "{s}"
        );
    }
    let got = parsed(
        "a.md",
        std::str::from_utf8(include_bytes!("fixtures/markdown/code.md")).unwrap(),
    );
    assert_eq!(got.elements.len(), 1);
    assert!(got.edges.is_empty());
}

#[test]
fn file_errors_never_return_partial_elements() {
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
        error(
            format!("{prefix}## Bad {{#bad}}\n<!-- letopis: {json} -->\nOops.\n").as_bytes(),
            InvalidMetadata,
        );
    }
    error(format!("{prefix}## A {{#a}}\n<!-- letopis: {{\"created_at\":2}} -->\n[self](./knowledge/../knowledge/a.md#a \"letopis:type=depends_on\")\n").as_bytes(),SelfLoop);
}

#[test]
fn nested_elements_and_orphan_metadata_are_rejected() {
    for s in [
        "## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n### B {#b}\n<!-- letopis: {\"created_at\":2} -->\n",
        "<!-- letopis: {\"created_at\":1} -->\n## B {#b}\n<!-- letopis: {\"created_at\":2} -->\n",
    ] {
        error(s.as_bytes(), NestedElement);
    }
    for s in [
        "intro\n<!-- letopis: {\"created_at\":1} -->\n",
        "## A {#a}\n\n<!-- letopis: {\"created_at\":1} -->\n",
        "## A\n<!-- letopis: {\"created_at\":1} -->\n",
    ] {
        error(s.as_bytes(), OrphanMetadata);
    }
}

#[test]
fn anchors_are_unique_across_real_headings() {
    error(
        b"## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n## B {#a}\n",
        DuplicateAnchor,
    );
    for anchor in ["L1-L2", "1bad", ""] {
        error(
            format!("## A {{#{anchor}}}\n<!-- letopis: {{\"created_at\":1}} -->\n").as_bytes(),
            InvalidAnchor,
        );
    }
}

#[test]
fn snapshot_metadata_preserves_full_oid_and_zero_created_at() {
    for n in [40, 64] {
        let oid = "a".repeat(n);
        let got = parsed(
            "a.markdown",
            &format!("<!-- letopis: {{\"created_at\":0,\"source_snapshot\":\"{oid}\"}} -->\nX"),
        );
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
    for s in [
        "<!-- letopis: {\n\"created_at\":1} -->\n",
        "<!-- letopis: {\"created_at\":1} --> trailing\n",
        "<!-- letopis: {\"created_at\":1} --> <!-- letopis: {\"created_at\":2} -->\n",
    ] {
        assert!(
            parse(&RepoPath::parse("a.md").unwrap(), s.as_bytes()).is_err(),
            "{s}"
        );
    }
}

#[test]
fn raw_html_content_does_not_declare_metadata() {
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
        let got = parsed("a.md", &whole(&format!("\n{raw}")));
        assert_eq!(got.elements.len(), 1, "{open}");
        assert!(
            got.elements[0].search_text.contains("created_at\":2"),
            "{open}"
        );
        assert!(
            parse(&RepoPath::parse("a.md").unwrap(), raw.as_bytes())
                .unwrap()
                .is_none(),
            "{open}"
        );
    }
}
