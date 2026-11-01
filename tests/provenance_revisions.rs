mod support;
use letopis::{
    domain::{SourceUri, TreeOid},
    service::{
        SnapshotErrorKind, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
    },
    wire_source_provenance,
};
use std::collections::BTreeSet;
use support::{
    git::{FORMATS, ObjectFormat, TempRepo},
    provenance::create_basis,
};

#[test]
fn compare_different_trees_by_one_addressed_pair() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let a = create_basis(repo.get_root(), &[("a", b"one\ntwo\n"), ("b", b"wrong")]);
        let b = create_basis(
            repo.get_root(),
            &[("a", b"changed\ntwo\n"), ("b", b"two\n")],
        );
        let service = wire_source_provenance(repo.get_root()).unwrap();
        let address = SourceUri::parse("a#L2-L2").unwrap();
        // When
        let actual = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&a),
                source: &address,
                revision: &b,
            })
            .unwrap();
        // Then
        assert_ne!(a, b);
        assert_eq!(actual, SourceRevisionComparison::Equal);
        let whole = SourceUri::parse("a").unwrap();
        assert_eq!(
            service
                .compare_revision(SourceRevisionRequest {
                    source_snapshot: Some(&a),
                    source: &whole,
                    revision: &b
                })
                .unwrap(),
            SourceRevisionComparison::Different
        );
        let cross = SourceUri::parse("b").unwrap();
        assert_eq!(
            service
                .compare_revision(SourceRevisionRequest {
                    source_snapshot: Some(&a),
                    source: &cross,
                    revision: &b
                })
                .unwrap(),
            SourceRevisionComparison::Different
        );
    }
}
#[test]
fn distinguish_missing_address_from_unknown_requested_tree() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(repo.get_root(), &[("a", b"one\n")]);
    let requested = create_basis(repo.get_root(), &[]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let source = SourceUri::parse("a#L1-L1").unwrap();
    // When / Then
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&basis),
                source: &source,
                revision: &requested
            })
            .unwrap(),
        SourceRevisionComparison::SourceNotFound
    );
    let missing = TreeOid::parse(&"a".repeat(40)).unwrap();
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&basis),
                source: &source,
                revision: &missing
            })
            .unwrap_err()
            .get_kind(),
        SnapshotErrorKind::NotFound
    );
}
#[test]
fn reject_missing_or_invalid_requested_revision_first() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let source = SourceUri::parse("https://host/path").unwrap();
    let missing = TreeOid::parse(&"a".repeat(40)).unwrap();
    // When / Then
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: None,
                source: &source,
                revision: &missing
            })
            .unwrap_err()
            .get_kind(),
        SnapshotErrorKind::NotFound
    );
    let commit = TreeOid::parse(&repo.get_oid(&["rev-parse", "HEAD"], None)).unwrap();
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: None,
                source: &source,
                revision: &commit
            })
            .unwrap_err()
            .get_kind(),
        SnapshotErrorKind::InvalidRequest
    );
    let foreign = TreeOid::parse(&"a".repeat(64)).unwrap();
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: None,
                source: &source,
                revision: &foreign
            })
            .unwrap_err()
            .get_kind(),
        SnapshotErrorKind::InvalidRequest
    );
}
#[test]
fn report_unavailable_basis_and_unfetched_remote_explicitly() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let tree = create_basis(repo.get_root(), &[]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let source = SourceUri::parse("a").unwrap();
    // When / Then
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: None,
                source: &source,
                revision: &tree
            })
            .unwrap(),
        SourceRevisionComparison::BasisUnavailable
    );
    let remote = SourceUri::parse("https://host/path").unwrap();
    assert_eq!(
        service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &remote,
                revision: &tree
            })
            .unwrap(),
        SourceRevisionComparison::Unfetched
    );
}

#[test]
fn report_incomplete_requested_range_without_crossing_source_bindings() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(repo.get_root(), &[("a", b"one\ntwo\n")]);
    let requested = create_basis(repo.get_root(), &[("a", b"one\n"), ("b", b"one\ntwo\n")]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let source = SourceUri::parse("a#L1-L2").unwrap();
    // When
    let actual = service
        .compare_revision(SourceRevisionRequest {
            source_snapshot: Some(&basis),
            source: &source,
            revision: &requested,
        })
        .unwrap();
    // Then
    assert_eq!(actual, SourceRevisionComparison::SourceNotFound);
}

#[test]
fn report_missing_or_invalid_basis_as_unavailable_for_valid_requested_revision() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let revision = create_basis(repo.get_root(), &[("a", b"one\n")]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let source = SourceUri::parse("a").unwrap();
    let commit = TreeOid::parse(&repo.get_oid(&["rev-parse", "HEAD"], None)).unwrap();
    let roots = [TreeOid::parse(&"a".repeat(40)).unwrap(), commit];
    for basis in roots {
        // When
        let actual = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&basis),
                source: &source,
                revision: &revision,
            })
            .unwrap();
        // Then
        assert_eq!(actual, SourceRevisionComparison::BasisUnavailable);
    }
}

#[test]
fn validate_all_requested_trees_without_a_source() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let valid = create_basis(repo.get_root(), &[]);
        let service = wire_source_provenance(repo.get_root()).unwrap();
        let foreign_length = match format {
            ObjectFormat::Sha1 => 64,
            ObjectFormat::Sha256 => 40,
        };
        let missing = TreeOid::parse(&"a".repeat(valid.as_str().len())).unwrap();
        let foreign = TreeOid::parse(&"a".repeat(foreign_length)).unwrap();
        let commit = TreeOid::parse(&repo.get_oid(&["rev-parse", "HEAD"], None)).unwrap();
        // When / Then: empty input and a complete empty tree need no source.
        assert!(service.validate_revisions(&BTreeSet::new()).is_ok());
        assert!(
            service
                .validate_revisions(&BTreeSet::from([valid.clone()]))
                .is_ok()
        );
        let result = service.validate_revisions(&BTreeSet::from([valid, missing]));
        assert_eq!(result.unwrap_err().get_kind(), SnapshotErrorKind::NotFound);
        for revision in [commit, foreign] {
            let result = service.validate_revisions(&BTreeSet::from([revision]));
            assert_eq!(
                result.unwrap_err().get_kind(),
                SnapshotErrorKind::InvalidRequest
            );
        }
    }
}

#[test]
fn validate_revisions_preserves_corrupt_root_failure() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let revision = create_basis(repo.get_root(), &[("a", b"basis\n")]);
        let replacement = create_basis(repo.get_root(), &[]);
        let service = wire_source_provenance(repo.get_root()).unwrap();
        repo.replace_loose_object(revision.as_str(), replacement.as_str());
        // When
        let result = service.validate_revisions(&BTreeSet::from([revision]));
        // Then
        assert_eq!(
            result.unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}
