use super::{
    CheckReport, CommandProblem, CommandResult, DocumentResult, ElementAddress, Inspection,
    SearchLink, SearchRequest,
};
use crate::domain::RepoPath;

/// Application-owned corpus validation capability.
pub trait Checker {
    /// Validate the given paths, or the whole corpus when empty.
    ///
    /// # Errors
    /// Reports command, refresh or infrastructure failures.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
/// Consistent metadata-only hybrid knowledge search.
pub trait Searcher {
    /// Refresh and search one pinned revision, observing baseline live knowledge freshness.
    ///
    /// # Errors
    /// Reports refresh, storage, integrity, or non-configuration filesystem failures.
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>>;
}
/// Application-owned metadata inspection capability.
pub trait Inspector {
    /// Inspect normalized addresses in one snapshot.
    ///
    /// # Errors
    /// Reports invalid, missing or unreadable elements and refresh failures.
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>;
}
/// Narrow readiness and execution capability of the embedding executor.
/// Implementations reuse the executor's configuration, durable attempts and publication
/// guards. Readiness never performs inference or retains a read transaction for launch.
pub trait EmbeddingExecutor {
    /// Determine whether configuration is ready and eligible durable work exists.
    ///
    /// # Errors
    /// Reports failures inspecting durable work.
    fn has_work(&mut self) -> CommandResult<bool>;
    /// Drain eligible work under the executor-owned lock.
    /// Missing/invalid configuration, changed identity and an occupied lock finish cleanly.
    ///
    /// # Errors
    /// Reports unexpected storage, coordination or embedding infrastructure failures.
    fn drain(&mut self) -> CommandResult<()>;
}
/// Nonblocking persistent embedding exclusion.
///
/// Every implementation grants ownership to at most one holder at a time across processes
/// and releases it when the returned lock is dropped.
pub trait EmbedLocker {
    /// Owned kernel lock released through RAII.
    type EmbedLock;
    /// Probe exclusive embedding ownership without waiting.
    ///
    /// # Errors
    /// Reports unsafe lock paths or filesystem/locking failures.
    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>>;
}
/// Detached drain process launch capability.
///
/// Every implementation returns without waiting for the launched process to finish.
pub trait DrainLauncher {
    /// Start the internal drain command without waiting for inference.
    ///
    /// # Errors
    /// Reports process setup or launch failures.
    fn spawn_drain(&self) -> CommandResult<()>;
}
/// Foreground triggering and direct execution of background work.
///
/// Every implementation reports launch problems as safe, nonfatal diagnostics where the
/// foreground result is already complete.
pub trait BackgroundRunner {
    /// Start eligible work, returning a safe nonfatal launch warning if needed.
    ///
    /// # Errors
    /// Reports failures inspecting durable readiness.
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>>;
    /// Delegate once to the embedding executor.
    ///
    /// # Errors
    /// Reports an unavailable executor or execution failures.
    fn drain(&mut self) -> CommandResult<()>;
}
