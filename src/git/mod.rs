//! In-process Git implementation of the application snapshot contract.
mod cleanup;
mod constants;
mod durability;
mod error;
mod hashing;
mod reader;
mod repository;
mod tree;
mod validation;

/// Command-owned Git write resources for foreground cancellation.
pub(crate) use cleanup::GitWriteCleanup;
/// In-process, foreground-owned snapshot and worktree implementation.
pub(crate) use repository::GitRepository;
