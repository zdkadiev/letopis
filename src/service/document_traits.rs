use super::{CapturedFile, Diagnostic, DocumentResult, FileStat, ParsedFile, RefreshOutcome};
use crate::domain::RepoPath;

/// Synchronous document publication operations.
///
/// Every implementation publishes a refresh atomically: readers see either the previous
/// projection or the complete new one, never a partial update.
pub trait Documents {
    /// Detect and atomically publish changed file projections.
    ///
    /// # Errors
    /// Reports unsafe paths, concurrent changes, or Git, filesystem and storage failures.
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome>;
}
/// Repository-owned discovery of Markdown candidate paths.
///
/// Every implementation reports only normalized root-relative paths and never reads file bodies.
pub trait CorpusFiles {
    /// List normalized, deduplicated candidates including deleted tracked files.
    ///
    /// # Errors
    /// Reports unsupported candidate paths or Git failures.
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>>;
}
/// Confined synchronous file observations and publication exclusion.
///
/// Every implementation stays beneath one pinned root, never follows symlinks out of it, and
/// fails rather than returning bytes it observed changing.
pub trait FileSystem {
    /// Exclusive kernel lock released when its guard is dropped.
    type IndexLock;
    /// Observe a regular file without reading its body; stable absence is `None`.
    ///
    /// # Errors
    /// Rejects symlinks, nonregular paths, observed changes and I/O failures.
    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>>;
    /// Capture exact regular-file bytes, checking identity and stat around the read.
    ///
    /// # Errors
    /// Rejects unsafe paths, observed replacements or edits, clock and I/O failures.
    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>>;
    /// Observe wall-clock nanoseconds since the Unix epoch with checked conversion.
    ///
    /// # Errors
    /// Reports clocks before the epoch or values outside the supported range.
    fn get_time_ns(&self) -> DocumentResult<i64>;
    /// Acquire the persistent exclusive index lock; never removes its file.
    ///
    /// # Errors
    /// Reports unsafe service paths, lock replacement or filesystem/locking failures.
    fn lock_index(&self) -> DocumentResult<Self::IndexLock>;
}

/// Pure shared Markdown projection and search-text codec.
///
/// Every implementation is deterministic and performs no I/O: equal bytes give equal results.
pub trait Markdown {
    /// Parse a whole file atomically; unmarked Markdown produces `None`.
    ///
    /// # Errors
    /// Returns safe local diagnostics for invalid UTF-8, metadata or graph markup.
    fn parse_file(
        &self,
        path: &RepoPath,
        bytes: &[u8],
    ) -> Result<Option<ParsedFile>, Vec<Diagnostic>>;
}
