//! Read-only knowledge graph foundations for Letopis.

use crate::adapters::git::GitCliAdapter;
pub mod adapters;
pub mod domain;

pub mod service;

/// Open immutable Git snapshot operations at the working copy containing `start`.
/// Requires Git 2.29 or newer, a UTF-8 checkout root, and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
///
/// # Errors
/// Returns a snapshot error when Git or the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(
    start: &std::path::Path,
) -> Result<impl service::ports::git::GitSnapshotPort, service::ports::git::SnapshotError> {
    GitCliAdapter::open(start)
}
