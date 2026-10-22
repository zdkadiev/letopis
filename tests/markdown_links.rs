mod support;
use letopis::{
    domain::{EdgeTarget, ElementKind, RepoPath, derive_element_kind},
    markdown::{DiagnosticKind, parse},
};
use support::{assert_error, build_whole_file, parse_file};

#[test]
fn only_inline_links_declare_edges() {
    // Arrange
    let s = build_whole_file(
        "[a](b.md \"letopis:type=includes\") [same](b.md \"letopis:type=includes\") [different](b.md \"letopis:type=depends_on\")\n![image](z.md \"letopis:type=includes\") [ref][r] [r][] [r] <https://example.com> [nav](https://example.com \"ordinary\")\n\n[r]: z.md \"letopis:type=includes\"\n",
    );
    // Act
    let got = parse_file("a.md", &s);
    let triples: Vec<_> = got
        .edges
        .iter()
        .map(|e| {
            (
                e.get_source().as_str(),
                e.get_target().as_str(),
                e.get_edge_type().as_str(),
            )
        })
        .collect();
    // Assert
    assert_eq!(got.edges.len(), 2);
    assert_eq!(
        derive_element_kind(&got.elements[0].element.id, &got.edges),
        ElementKind::Metavertex
    );
    assert_eq!(
        triples,
        vec![("a.md", "b.md", "depends_on"), ("a.md", "b.md", "includes")]
    );
}

#[test]
fn sources_are_normalized_without_loading_destinations() {
    // Arrange
    let text = &build_whole_file(
        "[code](./src/../src/a.rs#L010-L025 \"letopis:type=external\") [missing](knowledge/missing.md#x \"letopis:type=depends_on\")",
    );
    // Act
    let got = parse_file("knowledge/a.md", text);
    // Assert
    assert_eq!(got.edges.len(), 2);
    assert!(
        got.edges.iter().any(
            |e| matches!(e.get_target(),EdgeTarget::Source(s) if s.as_str()=="src/a.rs#L10-L25")
        )
    );
    assert!(got.edges.iter().any(
        |e| matches!(e.get_target(),EdgeTarget::Knowledge(k) if k.as_str()=="knowledge/missing.md#x")
    ));
    assert!(got.elements[0].element.metadata.source_snapshot.is_none());
}

#[test]
fn invalid_graph_titles_and_targets_fail_file() {
    // Arrange
    let path = RepoPath::parse("knowledge/a.md").unwrap();
    for title in [
        "letopis:type=",
        "letopis:type=Includes",
        "letopis:type=good;extra",
        "letopis:other=x",
    ] {
        let text = build_whole_file(&format!("[x](b.md \"{title}\")"));
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert_error(result, DiagnosticKind::InvalidLinkTitle);
    }
    // Arrange
    for (target, kind) in [
        ("../b.md", "depends_on"),
        ("/tmp/b.md", "depends_on"),
        ("b.md#L1-L2", "includes"),
        ("x.rs#L0-L1", "external"),
        ("x.rs#L2-L1", "external"),
        ("x.rs#anchor", "external"),
    ] {
        let text = build_whole_file(&format!("[x]({target} \"letopis:type={kind}\")"));
        // Act
        let result = parse(&path, text.as_bytes());
        // Assert
        assert_error(result, DiagnosticKind::InvalidTarget);
    }
    // Arrange
    let text = build_whole_file("[x](./knowledge/a.md \"letopis:type=depends_on\")");
    // Act
    let result = parse(&path, text.as_bytes());
    // Assert
    assert_error(result, DiagnosticKind::SelfLoop);
}

#[test]
fn links_outside_elements_are_only_navigation() {
    // Arrange
    let text = "[outside](../escape.md \"letopis:type=BAD\")\n\n## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n[ok](b.md \"letopis:type=includes\")\n\n## End\n[x](b.md \"letopis:type=other\")\n";
    // Act
    let got = parse_file("a.md", text);
    // Assert
    assert_eq!(got.edges.len(), 1);
    assert_eq!(got.edges.first().unwrap().get_source().as_str(), "a.md#a");
}

#[test]
fn image_alt_links_do_not_declare_edges() {
    // Arrange
    let text = &build_whole_file("![see [target](b.md \"letopis:type=includes\")](img.png)\n");
    // Act
    let got = parse_file("a.md", text);
    // Assert
    assert_eq!(got.elements[0].search_text, "see target");
    assert!(
        got.edges.is_empty(),
        "image alt cannot declare graph edges: {:?}",
        got.edges
    );
    assert_eq!(
        derive_element_kind(&got.elements[0].element.id, &got.edges),
        ElementKind::Vertex
    );
}

#[test]
fn links_wrapping_images_still_declare_edges() {
    // Arrange
    let text = &build_whole_file("[![alt](img.png)](b.md \"letopis:type=includes\")");
    // Act
    let got = parse_file("a.md", text);
    // Assert
    assert_eq!(got.edges.len(), 1);
    assert_eq!(got.edges.first().unwrap().get_target().as_str(), "b.md");
    assert_eq!(got.elements[0].search_text, "alt");
}
