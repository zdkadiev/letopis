//! In-process Git implementation of the application snapshot contract.
mod cleanup;
mod constants;
mod corpus;
mod durability;
mod error;
mod hashing;
mod objects;
mod reader;
mod refs;
mod repository;
mod tree;
mod validation;

/// Command-owned Git write resources for foreground cancellation.
pub(crate) use cleanup::GitWriteCleanup;
/// In-process, foreground-owned snapshot and corpus implementation.
pub(crate) use repository::GitRepository;
