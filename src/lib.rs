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
mod utils;
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

mod embedding;
use crate::embedding::{HttpProviders, Ollama, Openai};
use crate::service::{EmbeddingProvider, EmbeddingResult};

/// Construct the synchronous provider pair, capturing compatible credentials once.
/// Does not perform inference or require a key for native Ollama.
///
/// # Errors
/// Reserved for failures constructing the provider boundary.
pub fn open_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>> {
    let key = std::env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(Ollama::new(), Openai::new(key)))
}

mod filesystem;
use crate::filesystem::LocalFileSystem;
use crate::service::EmbeddingFiles;

/// Open confined embedding file reads and persistent drain exclusion.
/// The checkout directory is pinned and `.letopis` is protected with mode 0700.
///
/// # Errors
/// Reports unsafe service paths, observed replacements or filesystem failures.
pub fn open_embedding_files(root: &Path) -> EmbeddingResult<impl EmbeddingFiles + use<>> {
    LocalFileSystem::open(root)
}

use crate::markdown::MarkdownParser;
use crate::service::{EmbeddingService, Embeddings};

/// Assemble synchronous embeddings with explicit caller-owned storage.
///
/// # Errors
/// Reports confined filesystem or provider construction failures.
pub fn open_embeddings<S: Storage>(
    root: &Path,
    storage: S,
) -> EmbeddingResult<impl Embeddings + use<S>> {
    Ok(EmbeddingService::new(
        storage,
        LocalFileSystem::open(root)?,
        MarkdownParser,
        open_embedding_provider()?,
    ))
}
