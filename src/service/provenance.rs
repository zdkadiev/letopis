use super::utils::map_document_file;
use super::{
    BasisRetainer, DocumentError, DocumentErrorKind, DocumentResult, FileReader, ProvenanceIssue,
    ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SnapshotErrorKind,
    SnapshotState, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
    VerifiedGitSnapshots,
};
use crate::domain::{Sha256, SourceUri, TreeOid};
use std::path::Path;
use std::{collections::BTreeSet, fmt};

use observations::HistoricalObservations;

/// Compares exact addressed bytes and separately retains author-declared bases.
pub(crate) struct ProvenanceService<F> {
    git: Box<dyn VerifiedGitSnapshots>,
    filesystem: F,
}

impl<F> ProvenanceService<F> {
    /// Assemble collaborators without observing sources or changing Git refs.
    pub(crate) fn new(git: Box<dyn VerifiedGitSnapshots>, filesystem: F) -> Self {
        Self { git, filesystem }
    }
}

impl<F: fmt::Debug> fmt::Debug for ProvenanceService<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProvenanceService")
            .field("filesystem", &self.filesystem)
            .finish_non_exhaustive()
    }
}

impl<F: FileReader> SourceProvenance for ProvenanceService<F> {
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        for revision in revisions {
            self.verify_requested_revision(revision)?;
        }
        Ok(())
    }

    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        HistoricalObservations::new(self).observe_sources(request)
    }

    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        let mut observations = HistoricalObservations::new(self);
        requests
            .iter()
            .map(|request| observations.observe_sources(*request))
            .collect()
    }

    fn compare_revision(
        &self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        self.verify_requested_revision(request.revision)?;
        let Some(basis) = request.source_snapshot else {
            return Ok(SourceRevisionComparison::BasisUnavailable);
        };
        if basis != request.revision
            && self.git.tree_verify_by_oid(basis)? != SnapshotState::Available
        {
            return Ok(SourceRevisionComparison::BasisUnavailable);
        }
        if request.source.get_local().is_none() {
            return Ok(SourceRevisionComparison::Unfetched);
        }
        let historical = self.get_snapshot_hash(basis, request.source)?;
        let requested = self.get_snapshot_hash(request.revision, request.source)?;
        Ok(match (historical, requested) {
            (Some(left), Some(right)) if left == right => SourceRevisionComparison::Equal,
            (Some(_), Some(_)) => SourceRevisionComparison::Different,
            _ => SourceRevisionComparison::SourceNotFound,
        })
    }
}

impl<F> BasisRetainer for ProvenanceService<F> {
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self
                .git
                .tree_verify_by_oid(tree)
                .map_err(map_snapshot_error)?
                == SnapshotState::Available
            {
                self.git
                    .tree_retain_by_oid(tree)
                    .map_err(map_snapshot_error)?;
            }
        }
        Ok(())
    }
    fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self
                .git
                .tree_verify_by_oid(tree)
                .map_err(map_snapshot_error)?
                == SnapshotState::Available
            {
                self.git.tree_pin_by_oid(tree).map_err(map_snapshot_error)?;
            }
        }
        Ok(())
    }
}

impl<F: FileReader> ProvenanceService<F> {
    fn verify_requested_revision(&self, revision: &TreeOid) -> Result<(), SnapshotError> {
        match self.git.tree_verify_by_oid(revision)? {
            SnapshotState::Available => Ok(()),
            SnapshotState::Missing => Err(SnapshotError::new(
                SnapshotErrorKind::NotFound,
                "requested source revision is missing",
            )),
            SnapshotState::Invalid => Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "requested source revision is not a valid tree",
            )),
        }
    }

    fn get_snapshot_hash(
        &self,
        tree: &TreeOid,
        source: &SourceUri,
    ) -> Result<Option<Sha256>, SnapshotError> {
        let Some(local) = source.get_local() else {
            return Ok(None);
        };
        let bytes = match self
            .git
            .blob_get_by_tree_oid_and_path(tree, local.get_path())
        {
            Ok(bytes) => bytes,
            Err(error) if error.get_kind() == SnapshotErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(local.select_bytes(&bytes).map(Sha256::digest))
    }

    fn get_current_hash(&self, source: &SourceUri) -> DocumentResult<Option<Sha256>> {
        let Some(local) = source.get_local() else {
            return Ok(None);
        };
        let captured = map_document_file(
            self.filesystem
                .file_get_by_path(Path::new(local.get_path().as_str())),
        )?;
        Ok(captured
            .as_ref()
            .and_then(|file| local.select_bytes(&file.bytes))
            .map(Sha256::digest))
    }
}

fn build_observation(
    request: ProvenanceRequest<'_>,
    state: Option<SnapshotState>,
) -> ProvenanceObservation {
    let issue = match request.source_snapshot {
        Some(tree) if state != Some(SnapshotState::Available) => {
            Some(ProvenanceIssue::SnapshotUnavailable {
                snapshot: tree.clone(),
            })
        }
        None if !request.sources.is_empty() => Some(ProvenanceIssue::MissingSnapshot),
        _ => None,
    };
    ProvenanceObservation {
        flags: issue
            .as_ref()
            .map(|_| SearchFlag::SourceChanged)
            .into_iter()
            .collect(),
        issues: issue.into_iter().collect(),
    }
}

fn map_snapshot_error(cause: SnapshotError) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "source provenance Git operation failed",
        cause,
    )
}

mod observations {
    use super::{ProvenanceService, build_observation, map_snapshot_error};
    use crate::{
        domain::{Sha256, SourceUri, TreeOid},
        service::{
            DocumentResult, FileReader, ProvenanceIssue, ProvenanceObservation, ProvenanceRequest,
            SearchFlag, SnapshotState,
        },
    };
    use std::collections::{BTreeMap, BTreeSet};

    const HISTORICAL_ENTRY_LIMIT: usize = 4096;

    /// Operation-local immutable successes only; live captures and failures are never retained.
    #[derive(Debug)]
    pub(super) struct HistoricalObservations<'a, F> {
        provenance: &'a ProvenanceService<F>,
        available: BTreeSet<TreeOid>,
        hashes: BTreeMap<(TreeOid, SourceUri), Option<Sha256>>,
    }

    impl<'a, F: FileReader> HistoricalObservations<'a, F> {
        /// Start one empty operation-local history without performing I/O.
        pub(super) fn new(provenance: &'a ProvenanceService<F>) -> Self {
            Self {
                provenance,
                available: BTreeSet::new(),
                hashes: BTreeMap::new(),
            }
        }

        /// Observe one binding in demand order, reusing only successful historical work.
        ///
        /// # Errors
        /// Propagates the first root, addressed-object or confined live capture failure.
        pub(super) fn observe_sources(
            &mut self,
            request: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            let state = request
                .source_snapshot
                .map(|tree| self.verify_root(tree))
                .transpose()?;
            let mut observation = build_observation(request, state);
            let basis = request
                .source_snapshot
                .filter(|_| state == Some(SnapshotState::Available));
            for source in request.sources {
                if source.get_local().is_none() {
                    continue;
                }
                let historical = basis.map(|tree| self.get_hash(tree, source)).transpose()?;
                let current = self.provenance.get_current_hash(source)?;
                if current.is_none() || historical == Some(None) {
                    observation.issues.insert(ProvenanceIssue::SourceNotFound {
                        source: source.clone(),
                    });
                    observation.flags.insert(SearchFlag::SourceNotFound);
                } else if historical.is_some_and(|hash| hash != current) {
                    observation.flags.insert(SearchFlag::SourceChanged);
                }
            }
            Ok(observation)
        }

        fn verify_root(&mut self, tree: &TreeOid) -> DocumentResult<SnapshotState> {
            if self.available.contains(tree) {
                return Ok(SnapshotState::Available);
            }
            let state = self
                .provenance
                .git
                .tree_verify_by_oid(tree)
                .map_err(map_snapshot_error)?;
            if state == SnapshotState::Available && self.has_capacity() {
                self.available.insert(tree.clone());
            }
            Ok(state)
        }

        fn get_hash(
            &mut self,
            tree: &TreeOid,
            source: &SourceUri,
        ) -> DocumentResult<Option<Sha256>> {
            let key = (tree.clone(), source.clone());
            if let Some(hash) = self.hashes.get(&key) {
                return Ok(*hash);
            }
            let hash = self
                .provenance
                .get_snapshot_hash(tree, source)
                .map_err(map_snapshot_error)?;
            if self.has_capacity() {
                self.hashes.insert(key, hash);
            }
            Ok(hash)
        }

        fn has_capacity(&self) -> bool {
            self.available.len() + self.hashes.len() < HISTORICAL_ENTRY_LIMIT
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::service::{
        FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult,
    };

    mod batches {
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
                .borrow_mut()
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
                let service = create_service(fault);
                let tree = create_oid('a');
                let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
                let request = ProvenanceRequest {
                    source_snapshot: Some(&tree),
                    sources: &sources,
                };
                // When: the first batch encounters the configured failure.
                let error = service.observe_sources_batch(&[request]).unwrap_err();
                // Then: the failure propagates as an internal error.
                assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
                // When: the failure is repaired before a new operation.
                service.git.fault.set(GitFault::None);
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
            let service = create_service(GitFault::None);
            let first = create_oid('a');
            let second = create_oid('b');
            service
                .git
                .bytes
                .borrow_mut()
                .insert(first.clone(), b"basis\nold\n".to_vec());
            service
                .git
                .bytes
                .borrow_mut()
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
    }

    use super::ProvenanceService;
    use crate::{
        domain::{BlobOid, RepoPath, SourceUri, TreeOid},
        service::{
            BasisRetainer, CapturedFile, DocumentError, DocumentErrorKind, FileStat, GitSnapshots,
            ProvenanceRequest, RetainedTree, SnapshotEntry, SnapshotError, SnapshotErrorKind,
            SnapshotFile, SnapshotState, SnapshotVerifier, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest,
        },
    };
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        error::Error,
        io,
        ops::{Deref, DerefMut},
        rc::Rc,
    };

    #[test]
    fn full_audit_still_pins_available_roots() {
        // Given: one available author root.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        // When: exhaustive verification is explicitly requested.
        service
            .verify_snapshots(&BTreeSet::from([tree.clone()]))
            .unwrap();
        // Then: strong pin is used instead of lightweight retention.
        assert_eq!(*service.git.strong_pins.borrow(), [tree]);
        assert!(service.git.pins.borrow().is_empty());
    }

    #[test]
    fn leave_remote_unknown_without_file_reads_or_retention() {
        // Given: a remote source bound to an available historical tree.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let source = SourceUri::parse("https://host/path#L1-L2").unwrap();
        let sources = BTreeSet::from([source.clone()]);
        // When: the source is observed and compared with its declared revision.
        let observation = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            })
            .unwrap();
        let comparison = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            })
            .unwrap();
        // Then: remote bytes remain unknown without live reads or retention.
        assert!(observation.flags.is_empty());
        assert!(observation.issues.is_empty());
        assert_eq!(comparison, SourceRevisionComparison::Unfetched);
        assert_eq!(service.filesystem.reads.get(), 0);
        assert!(service.git.pins.borrow().is_empty());
    }

    #[test]
    fn verify_each_root_once_and_never_pin_during_read() {
        // Given: two local sources share one available historical root.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let source = SourceUri::parse("a#L1-L1").unwrap();
        let sources = BTreeSet::from([source.clone(), SourceUri::parse("b").unwrap()]);
        // When: the sources are observed, then one exact revision is compared.
        service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            })
            .unwrap();
        // Then: the shared root was verified once for the observation.
        assert_eq!(
            service.git.verified.borrow().as_slice(),
            std::slice::from_ref(&tree)
        );
        // Given: the recorder starts empty for an independent revision comparison.
        service.git.verified.borrow_mut().clear();
        // When: the source is compared with its exact declared revision.
        let comparison = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            })
            .unwrap();
        // Then: each operation verifies the root once and neither retains it.
        assert_eq!(comparison, SourceRevisionComparison::Equal);
        assert_eq!(*service.git.verified.borrow(), [tree]);
        assert!(service.git.pins.borrow().is_empty());
    }

    #[test]
    fn retain_each_unique_available_root_and_skip_missing_or_invalid() {
        // Given: duplicate available roots and missing or invalid roots.
        let service = create_service(GitFault::None);
        let available = create_oid('a');
        let missing = create_oid('b');
        let invalid = create_oid('c');
        service
            .git
            .states
            .borrow_mut()
            .insert(missing.clone(), SnapshotState::Missing);
        service
            .git
            .states
            .borrow_mut()
            .insert(invalid.clone(), SnapshotState::Invalid);
        let roots = BTreeSet::from([
            available.clone(),
            available.clone(),
            missing.clone(),
            invalid.clone(),
        ]);
        // When: the unique roots are retained.
        service.retain_snapshots(&roots).unwrap();
        // Then: all unique roots are verified and only the available root is retained.
        assert_eq!(
            *service.git.verified.borrow(),
            [available.clone(), missing, invalid]
        );
        assert_eq!(*service.git.pins.borrow(), [available]);
        assert_eq!(service.filesystem.reads.get(), 0);
    }

    #[test]
    fn propagate_retention_and_verification_failures_with_private_causes() {
        for fault in [GitFault::Verify, GitFault::Pin] {
            // Given: Git verification or retention fails with a private cause.
            let service = create_service(fault);
            let roots = BTreeSet::from([create_oid('a')]);
            // When: the author roots are retained.
            let error = service.retain_snapshots(&roots).unwrap_err();
            // Then: the public error preserves its private cause without exposing it.
            assert_private_document_error(&error);
        }
    }

    #[test]
    fn propagate_corruption_and_invalid_blob_instead_of_reporting_missing() {
        for fault in [GitFault::Verify, GitFault::Read, GitFault::ReadInvalid] {
            // Given: Git root verification or addressed blob reads fail.
            let service = create_service(fault);
            let tree = create_oid('a');
            let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
            // When: the local source is observed.
            let error = service
                .observe_sources(ProvenanceRequest {
                    source_snapshot: Some(&tree),
                    sources: &sources,
                })
                .unwrap_err();
            // Then: the failure remains an internal error with a private cause.
            assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
            assert!(error.source().is_some());
            assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
        }
    }

    #[test]
    fn propagate_unsafe_live_capture_without_exposing_private_cause() {
        // Given: live capture fails with a private cause.
        let mut service = create_service(GitFault::None);
        service.filesystem.fail = true;
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        // When: the local source is observed.
        let error = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            })
            .unwrap_err();
        // Then: the public error preserves its private cause without exposing it.
        assert_private_document_error(&error);
    }

    #[test]
    fn preserve_requested_revision_corruption_and_invalid_blob_classifications() {
        for (fault, expected) in [
            (GitFault::Verify, SnapshotErrorKind::InternalError),
            (GitFault::Read, SnapshotErrorKind::InternalError),
            (GitFault::ReadInvalid, SnapshotErrorKind::InvalidRequest),
        ] {
            // Given: Git verification or blob reads fail with a known classification.
            let service = create_service(fault);
            let tree = create_oid('a');
            let source = SourceUri::parse("a").unwrap();
            // When: the source is compared with its declared revision.
            let error = service
                .compare_revision(SourceRevisionRequest {
                    source_snapshot: Some(&tree),
                    source: &source,
                    revision: &tree,
                })
                .unwrap_err();
            // Then: the error classification survives without exposing the private cause.
            assert_eq!(error.get_kind(), expected);
            assert!(!error.get_message().contains("PRIVATE_PROVENANCE_CAUSE"));
            if expected == SnapshotErrorKind::InternalError {
                assert!(error.source().is_some());
            }
        }
    }

    #[derive(Clone, Copy)]
    enum GitFault {
        None,
        Verify,
        Read,
        ReadInvalid,
        Pin,
    }
    struct RecordedGit {
        states: RefCell<BTreeMap<TreeOid, SnapshotState>>,
        verified: RefCell<Vec<TreeOid>>,
        pins: RefCell<Vec<TreeOid>>,
        strong_pins: RefCell<Vec<TreeOid>>,
        fault: Cell<GitFault>,
        reads: Cell<usize>,
        bytes: RefCell<BTreeMap<TreeOid, Vec<u8>>>,
        states_sequence: RefCell<Vec<SnapshotState>>,
    }
    struct RecordedFiles {
        reads: Cell<usize>,
        fail: bool,
        changes: RefCell<Vec<Option<Vec<u8>>>>,
    }

    // Share the recorder between the boxed collaborator and independent test probes.
    struct RecordedProvenance {
        service: ProvenanceService<RecordedFiles>,
        git: Rc<RecordedGit>,
    }

    impl SnapshotVerifier for Rc<RecordedGit> {
        fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
            self.verified.borrow_mut().push(tree.clone());
            if matches!(self.fault.get(), GitFault::Verify) {
                return Err(create_git_failure());
            }
            if !self.states_sequence.borrow().is_empty() {
                return Ok(self.states_sequence.borrow_mut().remove(0));
            }
            Ok(self
                .states
                .borrow()
                .get(tree)
                .copied()
                .unwrap_or(SnapshotState::Available))
        }
    }
    impl GitSnapshots for Rc<RecordedGit> {
        fn tree_retain_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
            self.pins.borrow_mut().push(tree.clone());
            if matches!(self.fault.get(), GitFault::Pin) {
                return Err(create_git_failure());
            }
            Ok(())
        }
        fn tree_create(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
            panic!("provenance must not create trees")
        }
        fn blob_write(&self, _: &[u8]) -> Result<BlobOid, SnapshotError> {
            panic!("provenance must not write blobs")
        }
        fn tree_assemble(&self, _: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
            panic!("provenance must not create trees")
        }
        fn blob_get_by_tree_oid_and_path(
            &self,
            tree: &TreeOid,
            _: &RepoPath,
        ) -> Result<Vec<u8>, SnapshotError> {
            self.reads.set(self.reads.get() + 1);
            match self.fault.get() {
                GitFault::Read => Err(create_git_failure()),
                GitFault::ReadInvalid => Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "not a blob",
                )),
                _ => Ok(self
                    .bytes
                    .borrow()
                    .get(tree)
                    .cloned()
                    .unwrap_or_else(|| b"basis\n".to_vec())),
            }
        }
        fn tree_pin_by_oid(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
            self.strong_pins.borrow_mut().push(tree.clone());
            if matches!(self.fault.get(), GitFault::Pin) {
                return Err(create_git_failure());
            }
            Ok(RetainedTree::new(tree.clone()))
        }
    }
    impl FileReader for RecordedFiles {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }
        fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
            panic!("provenance must not stat sources")
        }
        fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            self.reads.set(self.reads.get() + 1);
            if self.fail {
                return Err(FileError::new_with_cause(
                    FileErrorKind::Io,
                    io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
                ));
            }
            let bytes = if self.changes.borrow().is_empty() {
                Some(b"basis\n".to_vec())
            } else {
                self.changes.borrow_mut().remove(0)
            };
            let Some(bytes) = bytes else {
                return Ok(None);
            };
            Ok(Some(CapturedFile {
                bytes,
                stat: FileStat {
                    size: 6,
                    mtime_ns: 1,
                    inode: 1,
                    device: 1,
                },
                checked_at_ns: 1,
            }))
        }
        fn file_get_identity_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<FileIdentity>> {
            panic!("unexpected file_get_identity_by_path")
        }
        fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            panic!("unexpected file_is_exist")
        }
        fn file_verify_identity_by_path(
            &self,
            _: &std::path::Path,
            _: Option<FileIdentity>,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_identity_by_path")
        }
        fn file_verify_distinct_by_paths(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_distinct_by_paths")
        }
        fn directory_get_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::os::fd::OwnedFd>> {
            panic!("unexpected directory_get_by_path")
        }
        fn lock_get_owner_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::num::NonZeroU32>> {
            panic!("unexpected lock_get_owner_by_path")
        }
    }
    impl FileLocker for RecordedFiles {
        type Lock = ();
        fn lock_acquire_by_path(&self, _path: &std::path::Path) -> FileResult<Self::Lock> {
            panic!("caller owns the index lock")
        }
        fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
            panic!("unexpected lock probe")
        }
    }
    impl Deref for RecordedProvenance {
        type Target = ProvenanceService<RecordedFiles>;

        fn deref(&self) -> &Self::Target {
            &self.service
        }
    }

    impl DerefMut for RecordedProvenance {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.service
        }
    }

    fn create_service(fault: GitFault) -> RecordedProvenance {
        let git = Rc::new(RecordedGit {
            states: RefCell::new(BTreeMap::new()),
            verified: RefCell::new(Vec::new()),
            pins: RefCell::new(Vec::new()),
            strong_pins: RefCell::new(Vec::new()),
            fault: Cell::new(fault),
            reads: Cell::new(0),
            bytes: RefCell::new(BTreeMap::new()),
            states_sequence: RefCell::new(Vec::new()),
        });
        let service = ProvenanceService::new(
            Box::new(Rc::clone(&git)),
            RecordedFiles {
                reads: Cell::new(0),
                fail: false,
                changes: RefCell::new(Vec::new()),
            },
        );
        RecordedProvenance { service, git }
    }
    fn create_oid(digit: char) -> TreeOid {
        TreeOid::parse(&digit.to_string().repeat(40)).unwrap()
    }
    fn create_git_failure() -> SnapshotError {
        SnapshotError::new_with_cause(
            "cannot access provenance tree",
            io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
        )
    }
    fn assert_private_document_error(error: &DocumentError) {
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
        assert!(!format!("{error:?}").contains("PRIVATE_PROVENANCE_CAUSE"));
        let mut cause = error.source();
        while let Some(current) = cause {
            if current.to_string().contains("PRIVATE_PROVENANCE_CAUSE") {
                return;
            }
            cause = current.source();
        }
        panic!("private cause was discarded");
    }
}
