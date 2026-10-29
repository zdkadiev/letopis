use super::{
    BasisRetainer, DocumentError, DocumentErrorKind, DocumentResult, FileSystem, GitSnapshots,
    ProvenanceIssue, ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError,
    SnapshotErrorKind, SnapshotState, SnapshotVerifier, SourceProvenance, SourceRevisionComparison,
    SourceRevisionRequest,
};
use crate::domain::{Sha256, SourceUri, TreeOid};
use std::collections::BTreeSet;

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
        let state = request
            .source_snapshot
            .map(|tree| self.git.verify_snapshot(tree))
            .transpose()
            .map_err(map_snapshot_error)?;
        let mut observation = build_observation(request, state);
        let basis = request
            .source_snapshot
            .filter(|_| state == Some(SnapshotState::Available));
        for source in request.sources {
            if source.get_local().is_none() {
                continue;
            }
            let Some(flag) = self.observe_local_source(basis, source)? else {
                continue;
            };
            if flag == SearchFlag::SourceNotFound {
                observation.issues.insert(ProvenanceIssue::SourceNotFound {
                    source: source.clone(),
                });
            }
            observation.flags.insert(flag);
        }
        Ok(observation)
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
        let historical = self.read_snapshot_hash(basis, request.source)?;
        let requested = self.read_snapshot_hash(request.revision, request.source)?;
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
    fn observe_local_source(
        &self,
        basis: Option<&TreeOid>,
        source: &SourceUri,
    ) -> DocumentResult<Option<SearchFlag>> {
        let historical = basis
            .map(|tree| self.read_snapshot_hash(tree, source))
            .transpose()
            .map_err(map_snapshot_error)?;
        let current = self.read_current_hash(source)?;
        if current.is_none() || historical == Some(None) {
            return Ok(Some(SearchFlag::SourceNotFound));
        }
        Ok(historical
            .filter(|hash| *hash != current)
            .map(|_| SearchFlag::SourceChanged))
    }

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

    fn read_snapshot_hash(
        &self,
        tree: &TreeOid,
        source: &SourceUri,
    ) -> Result<Option<Sha256>, SnapshotError> {
        let Some(local) = source.get_local() else {
            return Ok(None);
        };
        let bytes = match self.git.read_blob(tree, local.get_path()) {
            Ok(bytes) => bytes,
            Err(error) if error.get_kind() == SnapshotErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(local.select_bytes(&bytes).map(Sha256::digest))
    }

    fn read_current_hash(&self, source: &SourceUri) -> DocumentResult<Option<Sha256>> {
        let Some(local) = source.get_local() else {
            return Ok(None);
        };
        let captured = self.filesystem.read_file(local.get_path())?;
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
