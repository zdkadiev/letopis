mod support;
use letopis::{adapters::markdown::DiagnosticKind::*, domain::*};
use support::*;

#[test]
fn only_inline_links_declare_edges() {
    let s = whole(
        "[a](b.md \"letopis:type=includes\") [same](b.md \"letopis:type=includes\") [different](b.md \"letopis:type=depends_on\")\n![image](z.md \"letopis:type=includes\") [ref][r] [r][] [r] <https://example.com> [nav](https://example.com \"ordinary\")\n\n[r]: z.md \"letopis:type=includes\"\n",
    );
    let got = parsed("a.md", &s);
    assert_eq!(got.edges.len(), 2);
    assert_eq!(
        kind_for(&got.elements[0].element.id, &got.edges),
        ElementKind::Metavertex
    );
    let triples: Vec<_> = got
        .edges
        .iter()
        .map(|e| {
            (
                e.source().as_str(),
                e.target().as_str(),
                e.edge_type().as_str(),
            )
        })
        .collect();
    assert_eq!(
        triples,
        vec![("a.md", "b.md", "depends_on"), ("a.md", "b.md", "includes")]
    );
}

#[test]
fn sources_are_normalized_without_loading_destinations() {
    let got = parsed(
        "knowledge/a.md",
        &whole(
            "[code](./src/../src/a.rs#L010-L025 \"letopis:type=external\") [missing](knowledge/missing.md#x \"letopis:type=depends_on\")",
        ),
    );
    assert_eq!(got.edges.len(), 2);
    assert!(
        got.edges
            .iter()
            .any(|e| matches!(e.target(),EdgeTarget::Source(s) if s.as_str()=="src/a.rs#L10-L25"))
    );
    assert!(got.edges.iter().any(
        |e| matches!(e.target(),EdgeTarget::Knowledge(k) if k.as_str()=="knowledge/missing.md#x")
    ));
    assert!(got.elements[0].element.metadata.source_snapshot.is_none());
}

#[test]
fn invalid_graph_titles_and_targets_fail_file() {
    for title in [
        "letopis:type=",
        "letopis:type=Includes",
        "letopis:type=good;extra",
        "letopis:other=x",
    ] {
        error(
            whole(&format!("[x](b.md \"{title}\")")).as_bytes(),
            InvalidLinkTitle,
        );
    }
    for (target, kind) in [
        ("../b.md", "depends_on"),
        ("/tmp/b.md", "depends_on"),
        ("b.md#L1-L2", "includes"),
        ("x.rs#L0-L1", "external"),
        ("x.rs#L2-L1", "external"),
        ("x.rs#anchor", "external"),
    ] {
        error(
            whole(&format!("[x]({target} \"letopis:type={kind}\")")).as_bytes(),
            InvalidTarget,
        );
    }
    error(
        whole("[x](./knowledge/a.md \"letopis:type=depends_on\")").as_bytes(),
        SelfLoop,
    );
}

#[test]
fn links_outside_elements_are_only_navigation() {
    let got = parsed(
        "a.md",
        "[outside](../escape.md \"letopis:type=BAD\")\n\n## A {#a}\n<!-- letopis: {\"created_at\":1} -->\n[ok](b.md \"letopis:type=includes\")\n\n## End\n[x](b.md \"letopis:type=other\")\n",
    );
    assert_eq!(got.edges.len(), 1);
    assert_eq!(got.edges.first().unwrap().source().as_str(), "a.md#a");
}
