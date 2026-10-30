use super::{GitFault, create_oid, create_service};
use crate::{
    domain::{SourceUri, TreeOid},
    service::{
        DocumentErrorKind, ProvenanceIssue, ProvenanceRequest, SearchFlag, SnapshotState,
        SourceProvenance,
    },
};
use std::collections::BTreeSet;

#[test]
fn batch_reuses_exact_history_but_reads_each_live_binding() {
    // Given: two bindings of exactly the same historical address.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a#L1-L1").unwrap()]);
    let request = ProvenanceRequest {
        source_snapshot: Some(&tree),
        sources: &sources,
    };
    // When: both bindings are observed together.
    let results = service.observe_sources_batch(&[request, request]).unwrap();
    // Then: equal clean observations share history but capture live bytes twice.
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], results[1]);
    assert!(results[0].flags.is_empty());
    assert!(results[0].issues.is_empty());
    assert_eq!(service.git.verified.borrow().as_slice(), &[tree]);
    assert_eq!(service.git.reads.get(), 1);
    assert_eq!(service.filesystem.reads.get(), 2);
}

#[test]
fn batch_observation_detects_edit_between_live_reads() {
    // Given: the live file changes after the first capture.
    let service = create_service(GitFault::None);
    *service.filesystem.changes.borrow_mut() =
        vec![Some(b"basis\n".to_vec()), Some(b"edited\n".to_vec())];
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
    let request = ProvenanceRequest {
        source_snapshot: Some(&tree),
        sources: &sources,
    };
    // When: the two bindings are observed in order.
    let results = service.observe_sources_batch(&[request, request]).unwrap();
    // Then: only the second binding is changed; the historical read is shared.
    assert!(results[0].flags.is_empty());
    assert_eq!(
        results[1].flags,
        BTreeSet::from([SearchFlag::SourceChanged])
    );
    assert_eq!(service.git.reads.get(), 1);
    assert_eq!(service.filesystem.reads.get(), 2);
}

#[test]
fn batch_preserves_first_error_and_result_order() {
    // Given: the first binding has a changed source; the next has a missing basis.
    let mut service = create_service(GitFault::None);
    let tree = create_oid('a');
    let missing = create_oid('b');
    service
        .git
        .states
        .insert(missing.clone(), SnapshotState::Missing);
    *service.filesystem.changes.borrow_mut() = vec![Some(b"edit\n".to_vec()), None];
    let source = SourceUri::parse("a").unwrap();
    let sources = BTreeSet::from([source.clone()]);
    let requests = [
        ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        },
        ProvenanceRequest {
            source_snapshot: Some(&missing),
            sources: &sources,
        },
    ];
    // When: both observations succeed with diagnostic findings.
    let results = service.observe_sources_batch(&requests).unwrap();
    // Then: owner order and independent missing causes are preserved.
    assert_eq!(
        results[0].flags,
        BTreeSet::from([SearchFlag::SourceChanged])
    );
    assert_eq!(
        results[1].issues,
        BTreeSet::from([
            ProvenanceIssue::SnapshotUnavailable {
                snapshot: missing.clone()
            },
            ProvenanceIssue::SourceNotFound { source }
        ])
    );
    // Given: a live I/O failure at the first binding.
    service.filesystem.fail = true;
    service.git.verified.borrow_mut().clear();
    // When: the batch is attempted again.
    let error = service.observe_sources_batch(&requests).unwrap_err();
    // Then: no later root is demanded.
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert_eq!(service.git.verified.borrow().as_slice(), &[tree]);
}

#[test]
fn batch_does_not_cache_policy_or_io_failure() {
    for fault in [GitFault::Verify, GitFault::Read, GitFault::ReadInvalid] {
        // Given: a failed observation from Git policy, I/O or addressed type checks.
        let mut service = create_service(fault);
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        let error = service.observe_sources_batch(&[request]).unwrap_err();
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        // When: the failure is repaired before a new operation.
        service.git.fault = GitFault::None;
        let results = service.observe_sources_batch(&[request]).unwrap();
        // Then: successful current observation is not hidden by an error cache.
        assert!(results[0].flags.is_empty());
        assert_eq!(service.git.verified.borrow().len(), 2);
    }
}

#[test]
fn batch_releases_state_between_operations() {
    // Given: two equal bindings within each batch.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
    let request = ProvenanceRequest {
        source_snapshot: Some(&tree),
        sources: &sources,
    };
    // When: two batches and one singleton run on the same service.
    service.observe_sources_batch(&[request, request]).unwrap();
    service.observe_sources_batch(&[request, request]).unwrap();
    service.observe_sources(request).unwrap();
    // Then: each operation reads history anew and every binding captures live bytes.
    assert_eq!(service.git.verified.borrow().len(), 3);
    assert_eq!(service.git.reads.get(), 3);
    assert_eq!(service.filesystem.reads.get(), 5);
}

#[test]
fn batch_reclassifies_missing_and_invalid_roots() {
    for initial in [SnapshotState::Missing, SnapshotState::Invalid] {
        // Given: a previously unusable tree becomes available during the batch.
        let service = create_service(GitFault::None);
        *service.git.states_sequence.borrow_mut() = vec![initial, SnapshotState::Available];
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: the tree is demanded twice.
        let results = service.observe_sources_batch(&[request, request]).unwrap();
        // Then: the second binding sees the available tree.
        assert_eq!(
            results[0].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert!(results[1].flags.is_empty());
        assert_eq!(service.git.verified.borrow().len(), 2);
        assert_eq!(service.git.reads.get(), 1);
    }
}

#[test]
fn batch_keys_history_by_exact_tree_and_range() {
    // Given: distinct historical trees and line ranges have different bytes.
    let mut service = create_service(GitFault::None);
    let first = create_oid('a');
    let second = create_oid('b');
    service
        .git
        .bytes
        .insert(first.clone(), b"basis\nold\n".to_vec());
    service
        .git
        .bytes
        .insert(second.clone(), b"other\nbasis\n".to_vec());
    *service.filesystem.changes.borrow_mut() = vec![Some(b"basis\nbasis\n".to_vec()); 4];
    let line1 = BTreeSet::from([SourceUri::parse("a#L1-L1").unwrap()]);
    let line2 = BTreeSet::from([SourceUri::parse("a#L2-L2").unwrap()]);
    let requests = [
        ProvenanceRequest {
            source_snapshot: Some(&first),
            sources: &line1,
        },
        ProvenanceRequest {
            source_snapshot: Some(&second),
            sources: &line1,
        },
        ProvenanceRequest {
            source_snapshot: Some(&first),
            sources: &line2,
        },
        ProvenanceRequest {
            source_snapshot: Some(&second),
            sources: &line2,
        },
    ];
    // When: each exact tree/range is observed.
    let results = service.observe_sources_batch(&requests).unwrap();
    // Then: the independently chosen equal ranges are first/last, never the middle two.
    assert!(results[0].flags.is_empty());
    assert_eq!(
        results[1].flags,
        BTreeSet::from([SearchFlag::SourceChanged])
    );
    assert_eq!(
        results[2].flags,
        BTreeSet::from([SearchFlag::SourceChanged])
    );
    assert!(results[3].flags.is_empty());
    assert_eq!(service.git.verified.borrow().len(), 2);
    assert_eq!(service.git.reads.get(), 4);
}

#[test]
fn historical_cache_overflow_recomputes_without_rejecting() {
    // Given: one root plus more distinct addresses than the combined 4096-entry limit.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let sources: Vec<_> = (0..4096)
        .map(|n| BTreeSet::from([SourceUri::parse(&format!("a{n}")).unwrap()]))
        .collect();
    let mut requests: Vec<_> = sources
        .iter()
        .map(|sources| ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources,
        })
        .collect();
    requests.push(requests[4094]);
    requests.push(requests[4095]);
    // When: the last cached and first uncached addresses are demanded again.
    let results = service.observe_sources_batch(&requests).unwrap();
    // Then: the cache stays bounded, overflow recomputes and all results remain valid.
    assert_eq!(results.len(), 4098);
    assert!(results.iter().all(|result| result.flags.is_empty()));
    assert_eq!(service.git.verified.borrow().len(), 1);
    assert_eq!(service.git.reads.get(), 4097);
    assert_eq!(service.filesystem.reads.get(), 4098);
}

#[test]
fn singleton_and_batch_observe_the_same_sequence_of_live_edits() {
    // Given: independent captures with identical literal changes and a disappearance.
    let batch = create_service(GitFault::None);
    let singleton = create_service(GitFault::None);
    let changes = vec![Some(b"basis\n".to_vec()), Some(b"edited\n".to_vec()), None];
    *batch.filesystem.changes.borrow_mut() = changes.clone();
    *singleton.filesystem.changes.borrow_mut() = changes;
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
    let request = ProvenanceRequest {
        source_snapshot: Some(&tree),
        sources: &sources,
    };
    // When: the same sequence runs through the batch and singleton interfaces.
    let actual = batch.observe_sources_batch(&[request; 3]).unwrap();
    let sequential: Vec<_> = (0..3)
        .map(|_| singleton.observe_sources(request).unwrap())
        .collect();
    // Then: literal outcomes and both interfaces agree, without caching live bytes.
    assert!(actual[0].flags.is_empty());
    assert_eq!(actual[1].flags, BTreeSet::from([SearchFlag::SourceChanged]));
    assert_eq!(
        actual[2].flags,
        BTreeSet::from([SearchFlag::SourceNotFound])
    );
    assert_eq!(actual, sequential);
    assert_eq!(batch.git.reads.get(), 1);
    assert_eq!(singleton.git.reads.get(), 3);
}

#[test]
fn root_cache_overflow_also_recomputes_without_rejecting() {
    // Given: more available roots than the aggregate cache limit and no source bytes.
    let service = create_service(GitFault::None);
    let roots: Vec<_> = (0..4097)
        .map(|n| TreeOid::parse(&format!("{n:040x}")).unwrap())
        .collect();
    let sources = BTreeSet::new();
    let mut requests: Vec<_> = roots
        .iter()
        .map(|tree| ProvenanceRequest {
            source_snapshot: Some(tree),
            sources: &sources,
        })
        .collect();
    requests.push(requests[4095]);
    requests.push(requests[4096]);
    // When: the last cached and first uncached roots are repeated.
    let observations = service.observe_sources_batch(&requests).unwrap();
    // Then: root overflow recomputes while clean observations remain complete.
    assert_eq!(observations.len(), 4099);
    assert!(observations.iter().all(|result| result.flags.is_empty()));
    assert_eq!(service.git.verified.borrow().len(), 4098);
    assert_eq!(service.git.reads.get(), 0);
    assert_eq!(service.filesystem.reads.get(), 0);
}
