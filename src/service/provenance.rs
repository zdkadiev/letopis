use super::utils::map_document_file;
use super::{
    BasisRetainer, DocumentError, DocumentErrorKind, DocumentResult, FileReader, ProvenanceIssue,
    ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SnapshotErrorKind,
    SnapshotState, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
    VerifiedGitSnapshots,
};
use crate::domain::{Sha256, SourceUri, TreeOid};
use std::path::Path;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const HISTORICAL_ENTRY_LIMIT: usize = 4096;

/// Compares exact addressed bytes and separately retains author-declared bases.
pub(crate) struct ProvenanceService<F> {
    git: Box<dyn VerifiedGitSnapshots>,
    filesystem: F,
}

/// Operation-local immutable successes only; live captures and failures are never retained.
#[derive(Debug)]
struct HistoricalObservations<'a, F> {
    provenance: &'a ProvenanceService<F>,
    available: BTreeSet<TreeOid>,
    hashes: BTreeMap<(TreeOid, SourceUri), Option<Sha256>>,
}

impl<F> ProvenanceService<F> {
    /// Creates the service without observing sources or changing Git refs.
    ///
    /// # Arguments
    ///
    /// * `git` — Git snapshots used to verify revisions and read objects.
    /// * `filesystem` — File reader used for live captures.
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
            if self.git.tree_verify_by_oid(tree).map_err(|error| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "source provenance Git operation failed",
                    error,
                )
            })? == SnapshotState::Available
            {
                self.git.tree_retain_by_oid(tree).map_err(|error| {
                    DocumentError::new_with_cause(
                        DocumentErrorKind::InternalError,
                        "source provenance Git operation failed",
                        error,
                    )
                })?;
            }
        }
        Ok(())
    }

    fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self.git.tree_verify_by_oid(tree).map_err(|error| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "source provenance Git operation failed",
                    error,
                )
            })? == SnapshotState::Available
            {
                self.git.tree_pin_by_oid(tree).map_err(|error| {
                    DocumentError::new_with_cause(
                        DocumentErrorKind::InternalError,
                        "source provenance Git operation failed",
                        error,
                    )
                })?;
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

impl<'a, F: FileReader> HistoricalObservations<'a, F> {
    /// Creates an empty history without performing I/O.
    ///
    /// # Arguments
    ///
    /// * `provenance` — Service whose Git and filesystem access observes bindings.
    fn new(provenance: &'a ProvenanceService<F>) -> Self {
        Self {
            provenance,
            available: BTreeSet::new(),
            hashes: BTreeMap::new(),
        }
    }

    /// Observes one binding, reusing only successful historical results.
    ///
    /// # Arguments
    ///
    /// * `request` — Element basis and sources to observe.
    ///
    /// # Errors
    /// Reports the first root, object read or live capture failure.
    fn observe_sources(
        &mut self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        let state = request
            .source_snapshot
            .map(|tree| self.get_snapshot_state(tree))
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

    fn get_snapshot_state(&mut self, tree: &TreeOid) -> DocumentResult<SnapshotState> {
        if self.available.contains(tree) {
            return Ok(SnapshotState::Available);
        }
        let state = self
            .provenance
            .git
            .tree_verify_by_oid(tree)
            .map_err(|error| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "source provenance Git operation failed",
                    error,
                )
            })?;
        if state == SnapshotState::Available && self.is_capacity_available() {
            self.available.insert(tree.clone());
        }
        Ok(state)
    }

    fn get_hash(&mut self, tree: &TreeOid, source: &SourceUri) -> DocumentResult<Option<Sha256>> {
        let key = (tree.clone(), source.clone());
        if let Some(hash) = self.hashes.get(&key) {
            return Ok(*hash);
        }
        let hash = self
            .provenance
            .get_snapshot_hash(tree, source)
            .map_err(|error| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "source provenance Git operation failed",
                    error,
                )
            })?;
        if self.is_capacity_available() {
            self.hashes.insert(key, hash);
        }
        Ok(hash)
    }

    fn is_capacity_available(&self) -> bool {
        self.available.len() + self.hashes.len() < HISTORICAL_ENTRY_LIMIT
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

#[cfg(test)]
#[path = "provenance_test.rs"]
mod tests;
