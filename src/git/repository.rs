use super::{cleanup::GitWriteCleanup, corpus, objects, reader::ObjectReader, refs};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{
        CorpusLister, DocumentResult, GitSnapshots, RetainedTree, SnapshotEntry, SnapshotError,
        SnapshotErrorKind, SnapshotFile, SnapshotState, SnapshotVerifier,
    },
};
use std::{
    cell::RefCell,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

/// One foreground-owned repository and uncached raw reader, shared within a command.
#[derive(Clone, Debug)]
pub(crate) struct GitRepository {
    pub(super) repo: Rc<gix::Repository>,
    pub(super) reader: Rc<RefCell<ObjectReader>>,
    pub(super) root: PathBuf,
    pub(super) oid_len: usize,
    pub(super) cleanup: Arc<GitWriteCleanup>,
    alternates: PathBuf,
}
impl GitRepository {
    /// Discover a self-contained worktree without executable helpers or environment routing.
    pub(crate) fn open(start: &Path) -> Result<Self, SnapshotError> {
        Self::open_with_cleanup(start, Arc::new(GitWriteCleanup::new()))
    }
    /// Share the command's actual write-resource owner with the signal callback.
    pub(crate) fn open_with_cleanup(
        start: &Path,
        cleanup: Arc<GitWriteCleanup>,
    ) -> Result<Self, SnapshotError> {
        let mut permissions = gix::open::Permissions::default();
        permissions.env = gix::open::permissions::Environment::isolated();
        permissions.env.home = gix::sec::Permission::Allow;
        permissions.env.xdg_config_home = gix::sec::Permission::Allow;
        permissions.config.env = false;
        permissions.config.git_binary = false;
        let options = gix::open::Options::default()
            .permissions(permissions)
            .bail_if_untrusted(true)
            .config_overrides(["core.useReplaceRefs=false"]);
        let mut repo = gix::ThreadSafeRepository::discover_opts(
            start,
            Default::default(),
            gix::sec::trust::Mapping {
                full: options.clone(),
                reduced: options,
            },
        )
        .map_err(|error| classify_open_error(&error))?
        .to_thread_local();
        require_worktree_start(&repo, start)?;
        repo.objects.unset_object_cache();
        repo.objects.unset_pack_cache();
        let root = repo.workdir().ok_or_else(|| {
            SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "Git working copy required",
            )
        })?;
        let root = fs::canonicalize(root).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git root")
        })?;
        if root.to_str().is_none() {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "invalid Git root",
            ));
        }
        let oid_len = repo.object_hash().len_in_hex();
        let alternates = repo.common_dir().join("objects/info/alternates");
        require_absent_alternates(&alternates)?;
        let reader = Rc::new(RefCell::new(ObjectReader::new(&repo, oid_len)?));
        Ok(Self {
            repo: Rc::new(repo),
            reader,
            root,
            oid_len,
            cleanup,
            alternates,
        })
    }
    /// Return the discovered worktree root.
    pub(crate) fn get_root(&self) -> &Path {
        &self.root
    }
    /// Recheck policy at every operation; an earlier successful open is not authority.
    pub(super) fn require_self_contained_objects(&self) -> Result<(), SnapshotError> {
        require_absent_alternates(&self.alternates)
    }
}
impl GitSnapshots for GitRepository {
    fn retain(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.require_self_contained_objects()?;
        refs::Retainer { git: self }.retain(tree)
    }
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.create_tree(files)
    }
    fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.write_blob(bytes)
    }
    fn create_tree_from_blobs(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.create_tree_from_blobs(entries)
    }
    fn read_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.read_blob(tree, path)
    }
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        self.require_self_contained_objects()?;
        refs::Retainer { git: self }.pin(tree)
    }
}

impl SnapshotVerifier for GitRepository {
    fn verify_snapshot(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.require_self_contained_objects()?;
        match (objects::ObjectDb { git: self }).validate_tree(tree) {
            Ok(()) => Ok(SnapshotState::Available),
            Err(error) => match error.get_kind() {
                SnapshotErrorKind::NotFound => Ok(SnapshotState::Missing),
                SnapshotErrorKind::InvalidRequest => Ok(SnapshotState::Invalid),
                SnapshotErrorKind::InternalError => Err(error),
            },
        }
    }
}

impl CorpusLister for GitRepository {
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        corpus::list_candidates(&self.repo, &self.root)
    }
}
fn require_absent_alternates(path: &Path) -> Result<(), SnapshotError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "Git object alternates are unsupported; use a self-contained clone",
        )),
        Err(_) => Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "cannot inspect Git object alternates",
        )),
    }
}
fn require_worktree_start(repo: &gix::Repository, start: &Path) -> Result<(), SnapshotError> {
    let start = fs::canonicalize(start).map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "invalid Git start directory",
        )
    })?;
    for metadata in [repo.git_dir(), repo.common_dir()] {
        let metadata = fs::canonicalize(metadata).map_err(|_| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "invalid Git metadata directory",
            )
        })?;
        if start.starts_with(metadata) {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "Git working copy required",
            ));
        }
    }
    Ok(())
}
fn classify_open_error(error: &gix::Error) -> SnapshotError {
    if error
        .downcast_any_ref::<gix::discover::upwards::Error>()
        .is_some()
    {
        SnapshotError::new(
            SnapshotErrorKind::InvalidRequest,
            "Git working repository required",
        )
    } else {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "cannot open Git repository",
        )
    }
}
#[cfg(test)]
mod tests;
