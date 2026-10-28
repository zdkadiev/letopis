use super::{
    CheckReport, CommandProblem, CommandResult, Configuration, DocumentResult, ElementAddress,
    Inspection, SearchLink, SearchRequest, StatusReport, StorageResult, Traversal,
    TraversalRequest,
};
use crate::domain::RepoPath;
use std::{os::fd::OwnedFd, path::Path};

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
/// Filesystem capability required to initialize private service state.
///
/// Implementations create the service directory only when explicitly asked,
/// publish configuration without replacement and report durability work separately.
pub trait InitFiles: super::StatusFiles {
    /// Atomically publish complete bytes when `config.json` is absent.
    ///
    /// # Errors
    /// Rejects unsafe or nonregular destinations and reports write, link or sync
    /// failures. Existing files and another writer's winner are never replaced.
    fn install_config_if_absent(&self, bytes: &[u8]) -> DocumentResult<()>;
    /// Open or create the pinned private service directory.
    ///
    /// # Errors
    /// Reports unsafe/replaced paths, creation or permission failures.
    fn open_service_directory(&self) -> DocumentResult<OwnedFd>;
    /// Sync the existing service directory and its Git-root parent.
    ///
    /// # Errors
    /// Reports absent/replaced state or durability-sync failures without creating entries.
    fn sync_service_state(&self) -> DocumentResult<()>;
}

/// Storage capability required to initialize both service schemas.
pub trait InitStorage {
    /// Open or validate cache and vector schemas beneath one pinned directory.
    ///
    /// # Errors
    /// Reports unsafe paths, incompatible schemas, contention or database I/O failures.
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()>;
}

/// One repository initialization operation.
pub trait Initializer {
    /// Prepare service schemas and publish generated configuration without
    /// replacing existing valid state.
    ///
    /// # Errors
    /// Reports unsafe repository/config/storage state, invalid retained settings or
    /// an environment failure before success is observable.
    fn initialize(&mut self) -> CommandResult<()>;
}

/// Read the current validated embedding configuration without changing state.
///
/// Implementations observe only the non-creating configuration source and perform local
/// provider validation. They never expose raw bytes, identity, dimensions-derived data or
/// credentials, and never start readiness, vector resets, storage bootstrap or inference.
pub trait ConfigReader {
    /// Observe the current configuration, or its stable absence.
    ///
    /// # Errors
    /// Returns the complete nested document on success, or stable absence. A legacy
    /// flat document and every malformed or provider-rejected value return
    /// `EmbeddingFailed`; unreadable or unsafe files return `InternalError` with
    /// `cannot read embedding configuration`. Causes stay private and no partial
    /// configuration is returned.
    fn read_config(&self) -> CommandResult<Option<Configuration>>;
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
/// Application-owned metadata inspection from one retained Git tree.
///
/// Membership derives from current canonical Markdown, with duplicates returned once.
/// Knowledge hashes/lines and available source bytes match that tree. Known sources
/// with absent files or incomplete ranges have no hash; opaque URLs are never fetched.
/// Unknown or removed knowledge/source declarations fail the entire batch.
/// The final tree is retained before success; working files, Git index and HEAD stay intact.
pub trait Inspector {
    /// Inspect normalized addresses in one snapshot.
    ///
    /// # Errors
    /// Reports unknown elements, invalid current markup, unsafe paths, refresh or Git failures.
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>;
}
/// Narrow readiness and execution capability of the embedding executor.
/// Implementations reuse the executor's configuration, durable attempts and publication
/// guards. Readiness never performs inference or retains a read transaction for launch.
#[cfg_attr(test, mockall::automock)]
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
#[cfg_attr(test, mockall::automock)]
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
#[cfg_attr(test, mockall::automock)]
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
