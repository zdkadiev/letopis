use letopis::domain::{
    Anchor, ByteSpan, Edge, EdgeTarget, EdgeType, ElementKind, ElementMetadata, KnowledgeId,
    LineRange, Sha256, SourceUri, TreeOid, derive_element_kind,
};
use std::collections::BTreeSet;

#[test]
fn ids_and_sources_normalize_from_git_root() {
    // Arrange: each case supplies raw input and its canonical form.
    for (raw, want) in [
        ("./src/../src/a.rs#L010-L025", "src/a.rs#L10-L25"),
        ("src/A.rs", "src/A.rs"),
        ("src/тест.rs#L1-L1", "src/тест.rs#L1-L1"),
    ] {
        // Act
        let source = SourceUri::parse(raw).unwrap();
        // Assert
        assert_eq!(source.as_str(), want);
    }
    // Arrange
    for (raw, want) in [
        ("./knowledge/a.md#wal", "knowledge/a.md#wal"),
        ("A.markdown", "A.markdown"),
        ("a/../b.md", "b.md"),
    ] {
        // Act
        let id = KnowledgeId::parse(raw).unwrap();
        // Assert
        assert_eq!(id.as_str(), want);
    }
}

#[test]
fn anchors_and_ranges_reject_invalid_values() {
    // Arrange
    for raw in ["", "1start", "L01-L02", "has space", "a.b", "якорь"] {
        // Act
        let anchor = Anchor::parse(raw);
        // Assert
        assert!(anchor.is_err(), "{raw}");
    }
    // Arrange
    for raw in ["a", "WAL", "a_b-12", "L1", "Lx-Ly"] {
        // Act
        let anchor = Anchor::parse(raw);
        // Assert
        assert!(anchor.is_ok(), "{raw}");
    }
    // Arrange
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
        // Act
        let source = SourceUri::parse(raw);
        // Assert
        assert!(source.is_err(), "{raw:?}");
    }
    // Arrange
    for raw in ["src/a.rs", "a.md#L1-L2", "a.md#", "a.md#x#y"] {
        // Act
        let id = KnowledgeId::parse(raw);
        // Assert
        assert!(id.is_err(), "{raw}");
    }
    // Act: invalid byte and line ranges have independent constructors.
    let bytes = ByteSpan::new(2, 1);
    let zero_line = LineRange::new(0, 1);
    let reversed_lines = LineRange::new(2, 1);
    // Assert
    assert!(bytes.is_err());
    assert!(zero_line.is_err());
    assert!(reversed_lines.is_err());
}

#[test]
fn tree_oids_are_full_lowercase_hex() {
    // Arrange: these independent wire-length oracles cover both Git formats.
    for n in [40, 64] {
        let raw = "a".repeat(n);
        // Act
        let oid = TreeOid::parse(&raw).unwrap();
        // Assert
        assert_eq!(oid.as_str(), raw);
    }
    // Arrange
    for raw in [
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(64),
        format!("{}\n", "a".repeat(40)),
    ] {
        // Act
        let oid = TreeOid::parse(&raw);
        // Assert
        assert!(oid.is_err());
    }
}

#[test]
fn sha256_uses_exact_bytes() {
    // Arrange
    for (bytes, expected) in [
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
    ] {
        // Act
        let hash = Sha256::digest(bytes);
        // Assert
        assert_eq!(hash.to_hex(), expected);
    }
    // Arrange
    let lf = b"x\n";
    let crlf = b"x\r\n";
    // Act
    let hashes = (Sha256::digest(lf), Sha256::digest(crlf));
    // Assert
    assert_ne!(hashes.0, hashes.1);
}

#[test]
fn edge_identity_and_kind_are_derived() {
    // Arrange
    let a = KnowledgeId::parse("a.md").unwrap();
    let b = KnowledgeId::parse("b.md").unwrap();
    let edge = Edge::new(
        a.clone(),
        EdgeTarget::Knowledge(b.clone()),
        EdgeType::parse("includes").unwrap(),
    )
    .unwrap();
    // Act
    let mut edges = BTreeSet::from([edge.clone(), edge]);
    // Assert
    assert_eq!(edges.len(), 1);

    // Arrange
    let other = Edge::new(
        a.clone(),
        EdgeTarget::Knowledge(b.clone()),
        EdgeType::parse("depends_on").unwrap(),
    )
    .unwrap();
    // Act
    edges.insert(other);
    let source_kind = derive_element_kind(&a, &edges);
    let target_kind = derive_element_kind(&b, &edges);
    // Assert
    assert_eq!(edges.len(), 2);
    assert_eq!(source_kind, ElementKind::Metavertex);
    assert_eq!(target_kind, ElementKind::Vertex);

    // Arrange / Act: metadata accepts the epoch without a snapshot.
    let metadata = ElementMetadata {
        created_at: 0,
        source_snapshot: None,
    };
    // Assert
    assert_eq!(metadata.created_at, 0);
}

#[test]
fn edge_targets_and_self_loops_are_checked() {
    // Arrange
    let a = KnowledgeId::parse("a.md").unwrap();
    let b = KnowledgeId::parse("b.md").unwrap();
    for (target, kind, valid) in [
        (EdgeTarget::Knowledge(a.clone()), "depends_on", false),
        (
            EdgeTarget::Source(SourceUri::parse("a.md").unwrap()),
            "external",
            false,
        ),
        (
            EdgeTarget::Source(SourceUri::parse("x.rs").unwrap()),
            "includes",
            false,
        ),
        (EdgeTarget::Knowledge(b), "external", false),
        (
            EdgeTarget::Source(SourceUri::parse("x.rs#L1-L2").unwrap()),
            "external",
            true,
        ),
    ] {
        let edge_type = EdgeType::parse(kind).unwrap();
        // Act
        let edge = Edge::new(a.clone(), target, edge_type);
        // Assert
        assert_eq!(edge.is_ok(), valid);
    }
    // Arrange
    for raw in ["", "Includes", "a-b", "a b", "1a"] {
        // Act
        let kind = EdgeType::parse(raw);
        // Assert
        assert!(kind.is_err());
    }
}
