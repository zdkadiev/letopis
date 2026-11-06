use super::utils::map_document_error;
use super::{
    CacheDatabase, CheckReport, Checker, CommandError, CommandErrorKind, CommandResult,
    GraphDiagnostics, Refresher, SourceProvenance, utils::CheckObservations,
};
use crate::domain::RepoPath;
use std::{fmt, rc::Rc};

/// Reports current knowledge findings from one pinned cache view.
pub(crate) struct CheckService<D, P, C> {
    refresher: D,
    cache: Rc<C>,
    diagnostics: Box<dyn GraphDiagnostics>,
    provenance: P,
}

impl<D, P, C> fmt::Debug for CheckService<D, P, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CheckService")
            .finish_non_exhaustive()
    }
}

impl<D, P, C> CheckService<D, P, C> {
    /// Creates the service without refreshing or opening a read view.
    ///
    /// # Arguments
    ///
    /// * `refresher` — Refresher that brings the stored documents up to date before validation.
    /// * `cache` — Shared cache whose read transaction supplies the validation view.
    /// * `diagnostics` — Graph diagnostics read for structural findings.
    /// * `provenance` — Source provenance checked while collecting findings.
    pub(crate) fn new(
        refresher: D,
        cache: Rc<C>,
        diagnostics: Box<dyn GraphDiagnostics>,
        provenance: P,
    ) -> Self {
        Self {
            refresher,
            cache,
            diagnostics,
            provenance,
        }
    }
}

impl<D: Refresher, P: SourceProvenance, C: CacheDatabase> Checker for CheckService<D, P, C> {
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
        let outcome = if paths.is_empty() {
            self.refresher.refresh_verified(&[])
        } else {
            self.refresher.refresh(&[])
        }
        .map_err(|cause| map_document_error(cause, "cannot refresh corpus for check"))?;
        self.cache
            .with_read_tx(|| {
                let view: &dyn super::StorageRead = self.cache.as_ref();
                if paths.is_empty()
                    && view.project_get_by_id().map_err(|cause| {
                        CommandError::new_with_cause(
                            CommandErrorKind::InternalError,
                            "cannot read corpus for check",
                            cause,
                        )
                    })? != outcome.revision
                {
                    return Err(CommandError::new(
                        CommandErrorKind::InternalError,
                        "corpus changed during check; retry",
                    ));
                }
                let observations = CheckObservations {
                    storage_read: self.cache.as_ref(),
                    graph: self.diagnostics.as_ref(),
                    provenance: &self.provenance,
                };
                let errors = observations.collect_check_problems(paths)?;
                Ok(CheckReport { errors })
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot read corpus for check",
                    error,
                ),
            })
    }
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
