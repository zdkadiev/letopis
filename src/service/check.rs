use super::utils::{
    map_document_error_to_command_error, map_storage_error_to_check_corpus_command_error,
};
use super::{
    CacheDatabase, CheckReport, Checker, CommandError, CommandErrorKind, CommandResult,
    DocumentError, GraphDiagnostics, Refresher, SourceProvenance, TransactionError,
    utils::CheckObservations,
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
        .map_err(map_check_document_error_to_command_error)?;
        self.cache
            .with_read_tx(|| {
                let view: &dyn super::StorageRead = self.cache.as_ref();
                if paths.is_empty()
                    && view
                        .project_get_by_id()
                        .map_err(map_storage_error_to_check_corpus_command_error)?
                        != outcome.revision
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
            .map_err(map_transaction_error_to_check_corpus_command_error)
    }
}

/// Converts a document failure from the corpus refresh to a check command error.
///
/// # Arguments
///
/// * `cause` — Document failure from the refresh.
fn map_check_document_error_to_command_error(cause: DocumentError) -> CommandError {
    map_document_error_to_command_error(cause, "cannot refresh corpus for check")
}

/// Converts a transaction failure from the check read to a command error.
///
/// # Arguments
///
/// * `error` — Transaction failure from the pinned read.
fn map_transaction_error_to_check_corpus_command_error(
    error: TransactionError<CommandError>,
) -> CommandError {
    match error {
        TransactionError::Operation(error) => error,
        TransactionError::Storage(error) => map_storage_error_to_check_corpus_command_error(error),
    }
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
