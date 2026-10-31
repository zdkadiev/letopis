use super::{ProvenanceService, build_observation, map_snapshot_error};
use crate::{
    domain::{Sha256, SourceUri, TreeOid},
    service::{
        DocumentResult, FileSystem, GitSnapshots, ProvenanceIssue, ProvenanceObservation,
        ProvenanceRequest, SearchFlag, SnapshotState, SnapshotVerifier,
    },
};
use std::collections::{BTreeMap, BTreeSet};

const HISTORICAL_ENTRY_LIMIT: usize = 4096;

/// Operation-local immutable successes only; live captures and failures are never retained.
#[derive(Debug)]
pub(super) struct HistoricalObservations {
    available: BTreeSet<TreeOid>,
    hashes: BTreeMap<(TreeOid, SourceUri), Option<Sha256>>,
}

impl HistoricalObservations {
    /// Start one empty operation-local history without performing I/O.
    pub(super) fn new() -> Self {
        Self {
            available: BTreeSet::new(),
            hashes: BTreeMap::new(),
        }
    }

    /// Observe one binding in demand order, reusing only successful historical work.
    ///
    /// # Errors
    /// Propagates the first root, addressed-object or confined live capture failure.
    pub(super) fn observe_sources<G: GitSnapshots + SnapshotVerifier, F: FileSystem>(
        &mut self,
        provenance: &ProvenanceService<G, F>,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        let state = request
            .source_snapshot
            .map(|tree| self.verify_root(provenance, tree))
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
                .map(|tree| self.read_hash(provenance, tree, source))
                .transpose()?;
            let current = provenance.read_current_hash(source)?;
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

    fn verify_root<G: GitSnapshots + SnapshotVerifier, F>(
        &mut self,
        provenance: &ProvenanceService<G, F>,
        tree: &TreeOid,
    ) -> DocumentResult<SnapshotState> {
        if self.available.contains(tree) {
            return Ok(SnapshotState::Available);
        }
        let state = provenance
            .git
            .verify_snapshot(tree)
            .map_err(map_snapshot_error)?;
        if state == SnapshotState::Available && self.has_capacity() {
            self.available.insert(tree.clone());
        }
        Ok(state)
    }

    fn read_hash<G: GitSnapshots + SnapshotVerifier, F: FileSystem>(
        &mut self,
        provenance: &ProvenanceService<G, F>,
        tree: &TreeOid,
        source: &SourceUri,
    ) -> DocumentResult<Option<Sha256>> {
        let key = (tree.clone(), source.clone());
        if let Some(hash) = self.hashes.get(&key) {
            return Ok(*hash);
        }
        let hash = provenance
            .read_snapshot_hash(tree, source)
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
