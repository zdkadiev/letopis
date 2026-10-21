//! Read-only knowledge graph foundations for Letopis.
pub mod adapters;
pub mod domain;

pub mod service;

/// Open immutable Git snapshot operations at the working copy containing `start`.
/// Requires Git 2.29 or newer and a SHA-1 or SHA-256 working repository.
pub fn open_git_snapshots(
    start: &std::path::Path,
) -> Result<impl service::ports::git::GitSnapshotPort, service::ports::git::SnapshotError> {
    adapters::git::GitCliAdapter::open(start)
}
