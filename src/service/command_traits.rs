use super::{
    CheckReport, CommandProblem, CommandResult, DocumentResult, ElementAddress, Inspection,
    SearchLink, SearchRequest, StatusReport, Traversal, TraversalRequest,
};
use crate::domain::RepoPath;

/// Application-owned current corpus validation capability.
///
/// Implementations refresh the entire corpus before opening one pinned cache read.
/// All cache-derived findings share that view. Exact owner selectors filter findings
/// after full graph validation; they never restrict refresh or graph membership.
/// Source bases and bindings come from that view; live byte observations are read-only.
/// Missing sources or bases produce findings without removing knowledge.
pub trait Checker {
    /// Validate the given paths, or the whole corpus when empty.
    ///
    /// # Errors
    /// Reports refresh or infrastructure failures with private causes; no partial report
    /// is returned on failure. Validation findings are successful reports.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
/// Read the latest published diagnostics without refreshing or mutating state.
pub trait StatusReader {
    /// Observe one pinned database view, current configuration and busy locks.
    ///
    /// # Errors
    /// Reports storage, provenance or unsafe lock failures without a partial report.
    fn read_status(&mut self) -> CommandResult<StatusReport>;
}
/// Consistent metadata-only hybrid knowledge search.
pub trait Searcher {
    /// Refresh and search one pinned revision, observing live knowledge and source freshness.
    ///
    /// Source bindings and metadata share the ranking view; live bytes are observed separately.
    /// Flags are deduplicated X-1 values and links contain no source text or excerpts.
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

/// Consistent metadata-only traversal with live freshness observations.
///
/// Implementations refresh, synchronize embedding readiness before pinning one read
/// view, and build selected links without inference or fetching remote sources.
pub trait Traverser {
    /// Traverse and observe selected elements from one pinned publication revision.
    ///
    /// # Errors
    /// Returns safe refresh, storage, start-membership or observation errors without
    /// partial output. The pinned view is released on every return path.
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal>;
}
