use super::{
    CheckReport, Checker, CommandError, CommandErrorKind, CommandResult, DocumentError,
    GraphDiagnostics, Refresher, SourceProvenance, Storage, StorageError, StorageRead,
    check_problems::collect_check_problems,
};
use crate::domain::RepoPath;

/// Refreshes and validates current knowledge through one pinned cache view.
#[derive(Debug)]
pub(crate) struct CheckService<D, S, G, P> {
    refresher: D,
    storage: S,
    diagnostics: G,
    provenance: P,
}

impl<D, S, G, P> CheckService<D, S, G, P> {
    /// Assemble validation collaborators without refreshing or opening a read view.
    pub(crate) fn new(refresher: D, storage: S, diagnostics: G, provenance: P) -> Self {
        Self {
            refresher,
            storage,
            diagnostics,
            provenance,
        }
    }
}

impl<D: Refresher, S: Storage, G: GraphDiagnostics, P: SourceProvenance> Checker
    for CheckService<D, S, G, P>
{
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
        let outcome = if paths.is_empty() {
            self.refresher.refresh_verified()
        } else {
            self.refresher.refresh()
        }
        .map_err(map_refresh_error)?;
        let mut view = self.storage.begin_read_tx().map_err(map_storage_error)?;
        if paths.is_empty() && view.get_revision() != outcome.revision {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "corpus changed during check; retry",
            ));
        }
        let errors = collect_check_problems(&mut view, paths, &self.diagnostics, &self.provenance)?;
        Ok(CheckReport { errors })
    }
}

fn map_refresh_error(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot refresh corpus for check",
        cause,
    )
}
fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read corpus for check",
        cause,
    )
}
#[cfg(test)]
mod tests;
