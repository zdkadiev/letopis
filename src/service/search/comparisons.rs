use crate::{
    domain::{SourceUri, TreeOid},
    service::{SnapshotError, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest},
};
use std::{collections::BTreeMap, fmt};

const REVISION_COMPARISON_LIMIT: usize = 4096;

/// Results demanded by one eligibility computation, after requested-root validation.
pub(super) struct RevisionComparisons<'a> {
    provenance: &'a dyn SourceProvenance,
    results: BTreeMap<(Option<TreeOid>, SourceUri, TreeOid), SourceRevisionComparison>,
}

impl<'a> RevisionComparisons<'a> {
    /// Start one empty operation-local history without performing I/O.
    pub(super) fn new(provenance: &'a dyn SourceProvenance) -> Self {
        Self {
            provenance,
            results: BTreeMap::new(),
        }
    }

    /// Compare only when demanded, reusing verified immutable results within this search.
    /// Unavailable bases are rechecked because an object can appear before the next demand.
    ///
    /// # Errors
    /// Propagates comparison failures without retaining them.
    pub(super) fn compare_revision(
        &mut self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        let key = (
            request.source_snapshot.cloned(),
            request.source.clone(),
            request.revision.clone(),
        );
        if let Some(result) = self.results.get(&key) {
            return Ok(*result);
        }
        let result = self.provenance.compare_revision(request)?;
        if result != SourceRevisionComparison::BasisUnavailable
            && self.results.len() < REVISION_COMPARISON_LIMIT
        {
            self.results.insert(key, result);
        }
        Ok(result)
    }
}

impl fmt::Debug for RevisionComparisons<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevisionComparisons")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
