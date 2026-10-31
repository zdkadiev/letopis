use super::{
    BasisRetainer, DocumentError, DocumentErrorKind, DocumentResult, FileSystem, GitSnapshots,
    ProvenanceIssue, ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError,
    SnapshotErrorKind, SnapshotState, SnapshotVerifier, SourceProvenance, SourceRevisionComparison,
    SourceRevisionRequest,
};
use crate::domain::{Sha256, SourceUri, TreeOid};
use std::collections::BTreeSet;

mod observations;
use observations::HistoricalObservations;

/// Compares exact addressed bytes and separately retains author-declared bases.
#[derive(Debug)]
pub(crate) struct ProvenanceService<G, F> {
    git: G,
    filesystem: F,
}

impl<G, F> ProvenanceService<G, F> {
    /// Assemble collaborators without observing sources or changing Git refs.
    pub(crate) fn new(git: G, filesystem: F) -> Self {
        Self { git, filesystem }
    }
}

impl<G: GitSnapshots + SnapshotVerifier, F: FileSystem> SourceProvenance
    for ProvenanceService<G, F>
{
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
        HistoricalObservations::new().observe_sources(self, request)
    }

    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        let mut observations = HistoricalObservations::new();
        requests
            .iter()
            .map(|request| observations.observe_sources(self, *request))
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
        if basis != request.revision && self.git.verify_snapshot(basis)? != SnapshotState::Available
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

impl<G: GitSnapshots + SnapshotVerifier, F> BasisRetainer for ProvenanceService<G, F> {
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self.git.verify_snapshot(tree).map_err(map_snapshot_error)?
                == SnapshotState::Available
            {
                self.git.retain(tree).map_err(map_snapshot_error)?;
            }
        }
        Ok(())
    }
    fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        for tree in snapshots {
            if self.git.verify_snapshot(tree).map_err(map_snapshot_error)?
                == SnapshotState::Available
            {
                self.git.pin(tree).map_err(map_snapshot_error)?;
            }
        }
        Ok(())
    }
}

impl<G: GitSnapshots + SnapshotVerifier, F: FileSystem> ProvenanceService<G, F> {
    fn verify_requested_revision(&self, revision: &TreeOid) -> Result<(), SnapshotError> {
        match self.git.verify_snapshot(revision)? {
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
        let bytes = match self.git.get_blob(tree, local.get_path()) {
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
        let captured = self.filesystem.get_file(local.get_path())?;
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

#[cfg(test)]
mod tests;
