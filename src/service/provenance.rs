use super::utils::map_file_result_to_document_result;
use super::{
    DocumentError, DocumentErrorKind, DocumentResult, FileReader, ProvenanceIssue,
    ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SnapshotErrorKind,
    SnapshotRefsUpdater, SnapshotState, SourceProvenance, SourceRevisionComparison,
    SourceRevisionRequest, VerifiedGitSnapshots,
};
use crate::domain::{LocalSource, RepoPath, Sha256, SourceUri, TreeOid};
use std::path::Path;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const HISTORICAL_ENTRY_LIMIT: usize = 4096;
const HISTORICAL_PLAN_ADDRESS_BYTE_LIMIT: usize = 1024 * 1024;

/// Compares exact addressed bytes and separately retains author-declared bases.
pub(crate) struct ProvenanceService<F> {
    git: Box<dyn VerifiedGitSnapshots>,
    filesystem: F,
}

/// Operation-local immutable successes only; live captures and failures are never retained.
#[derive(Debug)]
struct HistoricalObservations<'service, 'request, F> {
    provenance: &'service ProvenanceService<F>,
    plan: Option<HistoricalPlan<'request>>,
    available: BTreeSet<TreeOid>,
    hashes: BTreeMap<(TreeOid, SourceUri), Option<Sha256>>,
}

/// Borrowed groups whose complete root and selector results fit the call budget.
#[derive(Debug)]
struct HistoricalPlan<'request> {
    groups: BTreeMap<(&'request TreeOid, &'request RepoPath), Vec<&'request SourceUri>>,
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
        HistoricalObservations::new(self, None).observe_sources(request)
    }

    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        let plan = if requests.len() > 1 {
            HistoricalPlan::build_plan(requests)
        } else {
            None
        };
        let mut observations = HistoricalObservations::new(self, plan);
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
        let historical = self.get_snapshot_hash_by_tree_and_source(basis, request.source)?;
        let requested =
            self.get_snapshot_hash_by_tree_and_source(request.revision, request.source)?;
        Ok(match (historical, requested) {
            (Some(left), Some(right)) if left == right => SourceRevisionComparison::Equal,
            (Some(_), Some(_)) => SourceRevisionComparison::Different,
            _ => SourceRevisionComparison::SourceNotFound,
        })
    }
}

impl<F> SnapshotRefsUpdater for ProvenanceService<F> {
    fn snapshot_refs_update_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self
                .git
                .tree_verify_by_oid(tree)
                .map_err(map_error_to_source_provenance_document_error)?
                == SnapshotState::Available
            {
                self.git
                    .ref_update_by_oid(tree)
                    .map_err(map_error_to_source_provenance_document_error)?;
            }
        }
        Ok(())
    }

    fn snapshot_sync_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self
                .git
                .tree_verify_by_oid(tree)
                .map_err(map_error_to_source_provenance_document_error)?
                == SnapshotState::Available
            {
                self.git
                    .tree_sync_by_oid(tree)
                    .map_err(map_error_to_source_provenance_document_error)?;
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

    fn get_snapshot_hash_by_tree_and_source(
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

    /// Gets all planned selector hashes from one verified historical file read.
    ///
    /// # Arguments
    ///
    /// * `tree` — Root of the planned group.
    /// * `sources` — Nonempty, unique local sources sharing the group's path.
    ///
    /// # Returns
    /// No outer value for a missing blob, otherwise one optional hash per selector.
    ///
    /// # Errors
    /// Propagates object, policy, type and I/O failures other than a missing blob.
    fn get_historical_hashes_by_tree_and_sources(
        &self,
        tree: &TreeOid,
        sources: &[&SourceUri],
    ) -> Result<Option<Vec<Option<Sha256>>>, SnapshotError> {
        #[expect(
            clippy::expect_used,
            reason = "HistoricalPlan groups contain at least one local source"
        )]
        let local = sources
            .first()
            .and_then(|source| source.get_local())
            .expect("planned local group");
        let bytes = match self
            .git
            .blob_get_by_tree_oid_and_path(tree, local.get_path())
        {
            Ok(bytes) => bytes,
            Err(error) if error.get_kind() == SnapshotErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let selectors: Vec<_> = sources
            .iter()
            .filter_map(|source| source.get_local())
            .map(LocalSource::get_lines)
            .collect();
        let byte_ranges = LocalSource::get_byte_ranges(&bytes, &selectors);
        let mut hashes = Vec::with_capacity(byte_ranges.len());
        for byte_range in byte_ranges {
            let selected = byte_range.and_then(|range| bytes.get(range));
            hashes.push(selected.map(Sha256::digest));
        }
        Ok(Some(hashes))
    }

    fn get_current_hash_by_source(&self, source: &SourceUri) -> DocumentResult<Option<Sha256>> {
        let Some(local) = source.get_local() else {
            return Ok(None);
        };
        let captured = map_file_result_to_document_result(
            self.filesystem
                .file_get_by_path(Path::new(local.get_path().as_str())),
        )?;
        Ok(captured
            .as_ref()
            .and_then(|file| local.select_bytes(&file.bytes))
            .map(Sha256::digest))
    }
}

impl<'service, 'request, F: FileReader> HistoricalObservations<'service, 'request, F> {
    /// Creates an empty history without performing I/O.
    ///
    /// # Arguments
    ///
    /// * `provenance` — Service whose Git and filesystem access observes bindings.
    /// * `plan` — Complete bounded group plan, or the sequential fallback.
    fn new(
        provenance: &'service ProvenanceService<F>,
        plan: Option<HistoricalPlan<'request>>,
    ) -> Self {
        Self {
            provenance,
            plan,
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
            .map(|tree| self.get_snapshot_state_by_tree(tree))
            .transpose()?;
        let mut observation = build_observation(request, state);
        let basis = request
            .source_snapshot
            .filter(|_| state == Some(SnapshotState::Available));
        for source in request.sources {
            if source.get_local().is_none() {
                continue;
            }
            let historical = basis
                .map(|tree| self.get_historical_hash_by_tree_and_source(tree, source))
                .transpose()?;
            let current = self.provenance.get_current_hash_by_source(source)?;
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

    fn get_snapshot_state_by_tree(&mut self, tree: &TreeOid) -> DocumentResult<SnapshotState> {
        if self.available.contains(tree) {
            return Ok(SnapshotState::Available);
        }
        let state = self
            .provenance
            .git
            .tree_verify_by_oid(tree)
            .map_err(map_error_to_source_provenance_document_error)?;
        if state == SnapshotState::Available && self.is_capacity_available() {
            self.available.insert(tree.clone());
        }
        Ok(state)
    }

    fn get_historical_hash_by_tree_and_source(
        &mut self,
        tree: &TreeOid,
        source: &SourceUri,
    ) -> DocumentResult<Option<Sha256>> {
        let key = (tree.clone(), source.clone());
        if let Some(hash) = self.hashes.get(&key) {
            return Ok(*hash);
        }
        if let Some(group) = self.plan.as_ref().and_then(|plan| {
            let local = source.get_local()?;
            plan.groups.get(&(tree, local.get_path()))
        }) {
            let hashes = self
                .provenance
                .get_historical_hashes_by_tree_and_sources(tree, group)
                .map_err(map_error_to_source_provenance_document_error)?;
            if let Some(hashes) = hashes {
                for (source, hash) in group.iter().zip(hashes) {
                    // Earlier exact absence survives a later successful group recovery.
                    self.hashes
                        .entry((tree.clone(), (*source).clone()))
                        .or_insert(hash);
                }
                return Ok(self.hashes.get(&key).copied().flatten());
            }
            if self.is_capacity_available() {
                self.hashes.insert(key, None);
            }
            return Ok(None);
        }
        let hash = self
            .provenance
            .get_snapshot_hash_by_tree_and_source(tree, source)
            .map_err(map_error_to_source_provenance_document_error)?;
        if self.is_capacity_available() {
            self.hashes.insert(key, hash);
        }
        Ok(hash)
    }

    fn is_capacity_available(&self) -> bool {
        self.available.len() + self.hashes.len() < HISTORICAL_ENTRY_LIMIT
    }
}

impl<'request> HistoricalPlan<'request> {
    fn build_plan(requests: &'request [ProvenanceRequest<'_>]) -> Option<Self> {
        let mut roots = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        let mut address_bytes = 0;
        for request in requests {
            let Some(tree) = request.source_snapshot else {
                continue;
            };
            if !roots.contains(tree) {
                if roots.len() + addresses.len() == HISTORICAL_ENTRY_LIMIT {
                    return None;
                }
                address_bytes = get_address_byte_count(address_bytes, tree.as_str().len(), 0)?;
                roots.insert(tree);
            }
            for source in request.sources {
                if source.get_local().is_none() || addresses.contains(&(tree, source)) {
                    continue;
                }
                if roots.len() + addresses.len() == HISTORICAL_ENTRY_LIMIT {
                    return None;
                }
                address_bytes = get_address_byte_count(
                    address_bytes,
                    tree.as_str().len(),
                    source.as_str().len(),
                )?;
                addresses.insert((tree, source));
            }
        }
        let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (tree, source) in addresses {
            if let Some(local) = source.get_local() {
                groups
                    .entry((tree, local.get_path()))
                    .or_default()
                    .push(source);
            }
        }
        // Admission reserves logical capacity for every root and exact result together.
        Some(Self { groups })
    }
}

fn get_address_byte_count(
    current: usize,
    tree_length: usize,
    source_length: usize,
) -> Option<usize> {
    let bytes = current
        .checked_add(tree_length)?
        .checked_add(source_length)?;
    (bytes <= HISTORICAL_PLAN_ADDRESS_BYTE_LIMIT).then_some(bytes)
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

/// Converts a Git failure to a source provenance document error.
///
/// # Arguments
///
/// * `cause` — Git failure from a provenance operation.
fn map_error_to_source_provenance_document_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "source provenance Git operation failed",
        cause,
    )
}

#[cfg(test)]
#[path = "provenance_test.rs"]
mod tests;
