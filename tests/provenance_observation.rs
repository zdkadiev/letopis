mod support;
use letopis::{
    domain::TreeOid,
    service::{
        DocumentErrorKind, ProvenanceIssue, ProvenanceRequest, SearchFlag, SourceProvenance,
    },
    wire_source_provenance,
};
use std::{collections::BTreeSet, fs};
use support::{
    git::{FORMATS, TempRepo},
    provenance::{create_basis, parse_sources},
};

#[test]
fn observe_exact_binary_range_and_ignore_unaddressed_edits() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let basis = create_basis(repo.get_root(), &[("src.bin", b"prefix\r\n\0abc\r\nlast")]);
        fs::write(repo.get_root().join("src.bin"), b"edited\n\0abc\r\nother").unwrap();
        let service = wire_source_provenance(repo.get_root()).unwrap();
        let sources = parse_sources(&["src.bin#L2-L2"]);
        // When / Then
        let equal = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&basis),
                sources: &sources,
            })
            .unwrap();
        assert_eq!(equal.flags, BTreeSet::new());
        assert_eq!(equal.issues, BTreeSet::new());
        fs::write(repo.get_root().join("src.bin"), b"edited\n\0xyz\r\nother").unwrap();
        let changed = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&basis),
                sources: &sources,
            })
            .unwrap();
        assert_eq!(changed.flags, BTreeSet::from([SearchFlag::SourceChanged]));
        assert_eq!(changed.issues, BTreeSet::new());
    }
}
#[test]
fn combine_changed_and_missing_sources_without_duplicates() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(
        repo.get_root(),
        &[("a.txt", b"old\n"), ("b.txt", b"one\ntwo\n")],
    );
    fs::write(repo.get_root().join("a.txt"), b"new\n").unwrap();
    fs::write(repo.get_root().join("b.txt"), b"one\n").unwrap();
    let sources = parse_sources(&["a.txt", "a.txt#L1-L1", "b.txt#L2-L2", "b.txt#L2-L2"]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    // When
    let actual = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        })
        .unwrap();
    // Then
    assert_eq!(
        actual.flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
    );
    assert_eq!(
        actual.issues,
        BTreeSet::from([ProvenanceIssue::SourceNotFound {
            source: parse_sources(&["b.txt#L2-L2"]).pop_first().unwrap()
        }])
    );
}
#[test]
fn distinguish_missing_basis_from_missing_source() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let sources = parse_sources(&["missing.txt"]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    // When
    let missing = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: None,
            sources: &sources,
        })
        .unwrap();
    // Then
    assert_eq!(
        missing.flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
    );
    assert!(missing.issues.contains(&ProvenanceIssue::MissingSnapshot));
    let oid = TreeOid::parse(&"a".repeat(40)).unwrap();
    let unavailable = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&oid),
            sources: &sources,
        })
        .unwrap();
    assert!(
        unavailable
            .issues
            .contains(&ProvenanceIssue::SnapshotUnavailable { snapshot: oid })
    );
    assert_eq!(unavailable.flags, missing.flags);
}
#[test]
fn observe_empty_and_unterminated_ranges() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(repo.get_root(), &[("empty", b""), ("lines", b"one\ntwo")]);
    fs::write(repo.get_root().join("empty"), b"").unwrap();
    fs::write(repo.get_root().join("lines"), b"one\ntwo").unwrap();
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let sources = parse_sources(&["empty", "lines#L2-L2"]);
    // When
    let actual = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        })
        .unwrap();
    // Then
    assert!(actual.flags.is_empty());
    let absent = parse_sources(&["empty#L1-L1", "lines#L2-L3"]);
    let actual = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &absent,
        })
        .unwrap();
    assert_eq!(actual.flags, BTreeSet::from([SearchFlag::SourceNotFound]));
    assert_eq!(actual.issues.len(), 2);
}
#[test]
fn diagnose_missing_historical_path_and_range() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(repo.get_root(), &[("a", b"one\n")]);
    fs::write(repo.get_root().join("a"), b"one\ntwo\n").unwrap();
    let sources = parse_sources(&["a#L2-L2", "new-file"]);
    fs::write(repo.get_root().join("new-file"), b"present now").unwrap();
    // When
    let actual = wire_source_provenance(repo.get_root())
        .unwrap()
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        })
        .unwrap();
    // Then
    assert_eq!(actual.flags, BTreeSet::from([SearchFlag::SourceNotFound]));
    assert_eq!(actual.issues.len(), 2);
}
#[test]
fn leave_remote_unknown_and_no_sources_unflagged() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let basis = create_basis(repo.get_root(), &[]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let sources = parse_sources(&["https://Host/a?x=1#L1-L2"]);
    // When
    let actual = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        })
        .unwrap();
    // Then
    assert!(actual.flags.is_empty());
    assert!(actual.issues.is_empty());
    let empty = BTreeSet::new();
    assert!(
        service
            .observe_sources(ProvenanceRequest {
                source_snapshot: None,
                sources: &empty
            })
            .unwrap()
            .flags
            .is_empty()
    );
}

#[test]
fn diagnose_declared_unavailable_roots_even_without_external_sources() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let service = wire_source_provenance(repo.get_root()).unwrap();
    let sources = BTreeSet::new();
    let commit = TreeOid::parse(&repo.get_oid(&["rev-parse", "HEAD"], None)).unwrap();
    let roots = [
        TreeOid::parse(&"a".repeat(40)).unwrap(),
        TreeOid::parse(&"a".repeat(64)).unwrap(),
        commit,
    ];
    for root in roots {
        // When
        let actual = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&root),
                sources: &sources,
            })
            .unwrap();
        // Then
        assert_eq!(actual.flags, BTreeSet::from([SearchFlag::SourceChanged]));
        assert_eq!(
            actual.issues,
            BTreeSet::from([ProvenanceIssue::SnapshotUnavailable { snapshot: root },])
        );
    }
}

#[test]
fn batch_ranges_keep_exact_bytes_and_separate_author_trees() {
    for format in FORMATS {
        // Given: overlapping line ranges and another author tree for the same path.
        let repo = TempRepo::new(format);
        let basis = create_basis(repo.get_root(), &[("code", b"one\r\n\0two\nlast")]);
        let other = create_basis(repo.get_root(), &[("code", b"other\r\n\0two\nlast")]);
        fs::write(repo.get_root().join("code"), b"one\r\n\0two\nchanged").unwrap();
        let sources = [
            parse_sources(&["code#L1-L1"]),
            parse_sources(&["code#L2-L2"]),
            parse_sources(&["code#L1-L2"]),
            parse_sources(&["code#L3-L4"]),
            parse_sources(&["code"]),
        ];
        let mut requests = Vec::new();
        for sources in &sources {
            requests.push(ProvenanceRequest {
                source_snapshot: Some(&basis),
                sources,
            });
        }
        requests.push(ProvenanceRequest {
            source_snapshot: Some(&other),
            sources: &sources[0],
        });
        requests.push(requests[1]);
        // When: all requests are observed in one batch.
        let actual = wire_source_provenance(repo.get_root())
            .unwrap()
            .observe_sources_batch(&requests)
            .unwrap();
        // Then: literal results catch range normalization, cross-tree reuse and truncation.
        let expected = [
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::from([SearchFlag::SourceNotFound]),
            BTreeSet::from([SearchFlag::SourceChanged]),
            BTreeSet::from([SearchFlag::SourceChanged]),
            BTreeSet::new(),
        ];
        assert_eq!(actual.len(), expected.len());
        for (observation, flags) in actual.iter().zip(expected) {
            assert_eq!(observation.flags, flags);
        }
        assert_eq!(
            actual[3].issues,
            BTreeSet::from([ProvenanceIssue::SourceNotFound {
                source: parse_sources(&["code#L3-L4"]).pop_first().unwrap(),
            }])
        );
        for index in [0, 1, 2, 4, 5, 6] {
            assert!(actual[index].issues.is_empty());
        }
    }
}

#[test]
fn next_batch_detects_corruption_after_successful_grouped_read() {
    for (format, request_count) in [
        (FORMATS[0], 1),
        (FORMATS[0], 2),
        (FORMATS[1], 1),
        (FORMATS[1], 2),
    ] {
        // Given: a successful multi-range observation through a long-lived service.
        let repo = TempRepo::new(format);
        let basis = create_basis(repo.get_root(), &[("code", b"one\ntwo\n")]);
        fs::write(repo.get_root().join("code"), b"one\ntwo\n").unwrap();
        let sources = parse_sources(&["code#L1-L1", "code#L2-L2"]);
        let request = [ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        }; 2];
        let service = wire_source_provenance(repo.get_root()).unwrap();
        let initial = service
            .observe_sources_batch(&request[..request_count])
            .unwrap();
        assert_eq!(initial.len(), request_count);
        for observation in initial {
            assert!(observation.flags.is_empty());
            assert!(observation.issues.is_empty());
        }
        let blob = repo.get_oid(&["rev-parse", &format!("{}:code", basis.as_str())], None);
        let replacement = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"wrong bytes"));
        // When: another valid object's compressed bytes replace the original identity.
        repo.replace_loose_object(&blob, &replacement);
        let result = service.observe_sources_batch(&request[..request_count]);
        // Then: earlier successful results cannot mask a new actual read's hash failure.
        assert_eq!(
            result.unwrap_err().get_kind(),
            DocumentErrorKind::InternalError
        );
    }
}

#[test]
fn next_batch_detects_deleted_blob_after_successful_grouped_read() {
    for (format, request_count) in [
        (FORMATS[0], 1),
        (FORMATS[0], 2),
        (FORMATS[1], 1),
        (FORMATS[1], 2),
    ] {
        // Given: two historical ranges have been observed successfully by one service.
        let repo = TempRepo::new(format);
        let basis = create_basis(repo.get_root(), &[("code", b"one\ntwo\n")]);
        fs::write(repo.get_root().join("code"), b"one\ntwo\n").unwrap();
        let sources = parse_sources(&["code#L1-L1", "code#L2-L2"]);
        let request = [ProvenanceRequest {
            source_snapshot: Some(&basis),
            sources: &sources,
        }; 2];
        let service = wire_source_provenance(repo.get_root()).unwrap();
        let initial = service
            .observe_sources_batch(&request[..request_count])
            .unwrap();
        assert_eq!(initial.len(), request_count);
        for observation in initial {
            assert!(observation.flags.is_empty());
            assert!(observation.issues.is_empty());
        }
        let blob = repo.get_oid(&["rev-parse", &format!("{}:code", basis.as_str())], None);
        // When: the tree still exists but its referenced blob is removed.
        fs::remove_file(repo.build_object_path(&blob)).unwrap();
        let result = service.observe_sources_batch(&request[..request_count]);
        // Then: a tree referencing a missing blob is a typed integrity failure.
        assert_eq!(
            result.unwrap_err().get_kind(),
            DocumentErrorKind::InternalError
        );
    }
}
