use letopis::domain::*;
use std::collections::BTreeSet;

#[test]
fn ids_and_sources_normalize_from_git_root() {
    for (raw, want) in [
        ("./src/../src/a.rs#L010-L025", "src/a.rs#L10-L25"),
        ("src/A.rs", "src/A.rs"),
        ("src/тест.rs#L1-L1", "src/тест.rs#L1-L1"),
    ] {
        assert_eq!(SourceUri::parse(raw).unwrap().as_str(), want);
    }
    for (raw, want) in [
        ("./knowledge/a.md#wal", "knowledge/a.md#wal"),
        ("A.markdown", "A.markdown"),
        ("a/../b.md", "b.md"),
    ] {
        assert_eq!(KnowledgeId::parse(raw).unwrap().as_str(), want);
    }
}

#[test]
fn anchors_and_ranges_reject_invalid_values() {
    for raw in ["", "1start", "L01-L02", "has space", "a.b", "якорь"] {
        assert!(Anchor::parse(raw).is_err(), "{raw}");
    }
    for raw in ["a", "WAL", "a_b-12", "L1", "Lx-Ly"] {
        assert!(Anchor::parse(raw).is_ok(), "{raw}");
    }
    for raw in [
        "/tmp/a.rs",
        "../a.rs",
        "a/../../b.rs",
        "a.rs#L0-L2",
        "a.rs#L3-L2",
        "a.rs#L1-Lx",
        "a.rs#L9999999999999999999999999999-L2",
        "",
        ".",
        "a\n.rs",
        "a.rs#L1-L2#bad",
    ] {
        assert!(SourceUri::parse(raw).is_err(), "{raw:?}");
    }
    for raw in ["src/a.rs", "a.md#L1-L2", "a.md#", "a.md#x#y"] {
        assert!(KnowledgeId::parse(raw).is_err(), "{raw}");
    }
    assert!(ByteSpan::new(2, 1).is_err());
    assert!(LineRange::new(0, 1).is_err());
    assert!(LineRange::new(2, 1).is_err());
}

#[test]
fn tree_oids_are_full_lowercase_hex() {
    for n in [40, 64] {
        assert_eq!(
            TreeOid::parse(&"a".repeat(n)).unwrap().as_str(),
            "a".repeat(n)
        );
    }
    for s in [
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(64),
        format!("{}\n", "a".repeat(40)),
    ] {
        assert!(TreeOid::parse(&s).is_err());
    }
}

#[test]
fn sha256_uses_exact_bytes() {
    assert_eq!(
        Sha256::digest(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        Sha256::digest(b"").to_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_ne!(Sha256::digest(b"x\n"), Sha256::digest(b"x\r\n"));
}

#[test]
fn edge_identity_and_kind_are_derived() {
    let a = KnowledgeId::parse("a.md").unwrap();
    let b = KnowledgeId::parse("b.md").unwrap();
    let e = Edge::new(
        a.clone(),
        EdgeTarget::Knowledge(b.clone()),
        EdgeType::parse("includes").unwrap(),
    )
    .unwrap();
    let mut edges = BTreeSet::from([e.clone(), e]);
    assert_eq!(edges.len(), 1);
    edges.insert(
        Edge::new(
            a.clone(),
            EdgeTarget::Knowledge(b.clone()),
            EdgeType::parse("depends_on").unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(edges.len(), 2);
    assert_eq!(kind_for(&a, &edges), ElementKind::Metavertex);
    assert_eq!(kind_for(&b, &edges), ElementKind::Vertex);
    let metadata = ElementMetadata {
        created_at: 0,
        source_snapshot: None,
    };
    assert_eq!(metadata.created_at, 0);
}

#[test]
fn edge_targets_and_self_loops_are_checked() {
    let a = KnowledgeId::parse("a.md").unwrap();
    let b = KnowledgeId::parse("b.md").unwrap();
    assert!(
        Edge::new(
            a.clone(),
            EdgeTarget::Knowledge(a.clone()),
            EdgeType::parse("depends_on").unwrap()
        )
        .is_err()
    );
    assert!(
        Edge::new(
            a.clone(),
            EdgeTarget::Source(SourceUri::parse("a.md").unwrap()),
            EdgeType::parse("external").unwrap()
        )
        .is_err()
    );
    assert!(
        Edge::new(
            a.clone(),
            EdgeTarget::Source(SourceUri::parse("x.rs").unwrap()),
            EdgeType::parse("includes").unwrap()
        )
        .is_err()
    );
    assert!(
        Edge::new(
            a.clone(),
            EdgeTarget::Knowledge(b),
            EdgeType::parse("external").unwrap()
        )
        .is_err()
    );
    assert!(
        Edge::new(
            a,
            EdgeTarget::Source(SourceUri::parse("x.rs#L1-L2").unwrap()),
            EdgeType::parse("external").unwrap()
        )
        .is_ok()
    );
    for raw in ["", "Includes", "a-b", "a b", "1a"] {
        assert!(EdgeType::parse(raw).is_err());
    }
}
