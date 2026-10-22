//! Read-only knowledge graph foundations for Letopis.

use crate::git::GitCli;
use crate::service::{GitSnapshots, SnapshotError};
use std::path::Path;

pub mod domain;
mod git;
pub mod markdown;

pub mod service;

/// Open immutable Git snapshot operations at the working copy containing `start`.
/// Requires Git 2.29 or newer, a UTF-8 checkout root, and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
///
/// # Errors
/// Returns a snapshot error when Git or the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(start: &Path) -> Result<impl GitSnapshots, SnapshotError> {
    GitCli::open(start)
}

mod storage;
use crate::service::{Storage, StorageResult};
use crate::storage::SqliteStorage;

/// Open the disk cache and independent vector database.
///
/// # Errors
/// Returns an invalid request for aliased or invalid paths, or an internal error
/// for unsupported capabilities, incompatible schemas, contention or I/O errors.
pub fn open_storage(cache: &Path, vectors: &Path) -> StorageResult<impl Storage + use<>> {
    SqliteStorage::open(cache, vectors)
}
