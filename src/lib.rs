//! Read-only knowledge graph foundations for Letopis.

use crate::filesystem::LocalFileSystem;
use crate::git::GitCli;
use crate::markdown::MarkdownParser;
use crate::service::{
    DocumentError, DocumentErrorKind, DocumentResult, DocumentService, Documents, FileSystem,
    GitSnapshots, SnapshotError, Storage, StorageResult,
};
use crate::storage::SqliteStorage;
use std::path::Path;

pub mod domain;
mod filesystem;
mod git;
pub mod markdown;
mod storage;

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

/// Open the disk cache and independent vector database.
///
/// # Errors
/// Returns an invalid request for aliased or invalid paths, or an internal error
/// for unsupported capabilities, incompatible schemas, contention or I/O errors.
pub fn open_storage(cache: &Path, vectors: &Path) -> StorageResult<impl Storage + use<>> {
    SqliteStorage::open(cache, vectors)
}

/// Open confined regular-file observations and persistent index locking.
///
/// # Errors
/// Reports an inaccessible or non-directory root, including a symlink root.
pub fn open_filesystem(root: &Path) -> DocumentResult<impl FileSystem + use<>> {
    LocalFileSystem::open(root)
}

/// Open document refresh operations for the working copy containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths, or storage/I/O failures.
pub fn open_documents(start: &Path) -> DocumentResult<impl Documents + use<>> {
    let git = GitCli::open(start).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open document repository",
            cause,
        )
    })?;
    let filesystem = LocalFileSystem::open(git.get_root())?;
    let directory = filesystem.open_service_directory()?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory)
        .map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot open document storage",
                cause,
            )
        })?;
    Ok(DocumentService::new(
        git,
        filesystem,
        MarkdownParser::new(),
        storage,
    ))
}
