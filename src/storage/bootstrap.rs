use super::SqliteStorage;
use crate::service::{InitStorage, StorageResult};
use rustix::fd::OwnedFd;
use std::path::Path;

/// Stateless access to the existing schema opener.
#[derive(Debug, Default)]
pub(crate) struct SqliteBootstrap;

impl InitStorage for SqliteBootstrap {
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()> {
        SqliteStorage::open_confined(directory, pinned).map(drop)
    }
}
