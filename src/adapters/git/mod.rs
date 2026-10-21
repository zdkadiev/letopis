mod objects;
mod process;
mod refs;
#[cfg(test)]
mod tests;
use crate::service::ports::git::{SnapshotError, SnapshotErrorKind::InternalError};
use process::{GitProcess, check_version};
use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
pub(crate) struct GitCliAdapter {
    root: PathBuf,
    process: GitProcess,
    oid_len: usize,
}
impl GitCliAdapter {
    pub(crate) fn open(start: &Path) -> Result<Self, SnapshotError> {
        Self::open_with_process(start, GitProcess::system())
    }
    fn open_with_process(start: &Path, process: GitProcess) -> Result<Self, SnapshotError> {
        check_version(&process.run(start, &["--version".into()], None)?)?;
        let raw = process.run(start, &["rev-parse".into(), "--show-toplevel".into()], None)?;
        let root = PathBuf::from(
            String::from_utf8(raw)
                .map_err(|_| SnapshotError::new(InternalError, "invalid Git root"))?
                .strip_suffix('\n')
                .ok_or_else(|| SnapshotError::new(InternalError, "invalid Git root response"))?,
        );
        if !root.is_absolute() {
            return Err(SnapshotError::new(InternalError, "invalid Git root"));
        }
        let inside = process.run(
            &root,
            &["rev-parse".into(), "--is-inside-work-tree".into()],
            None,
        )?;
        if inside != b"true\n" {
            return Err(SnapshotError::new(
                InternalError,
                "Git working copy required",
            ));
        }
        let format = process.run(
            &root,
            &[OsString::from("rev-parse"), "--show-object-format".into()],
            None,
        )?;
        let oid_len = match format.as_slice() {
            b"sha1\n" => 40,
            b"sha256\n" => 64,
            _ => {
                return Err(SnapshotError::new(
                    InternalError,
                    "unsupported Git object format",
                ));
            }
        };
        let git = Self {
            root,
            process,
            oid_len,
        };
        git.require_self_contained_objects()?;
        Ok(git)
    }
    // Git resolves this path through the actual gitdir/common-dir layout.
    // Reject presence, including empty/inactive files, without reading donor paths.
    // Recheck at operation entry: alternates may have been added since open.
    fn require_self_contained_objects(&self) -> Result<(), SnapshotError> {
        let raw = self.process.run(
            &self.root,
            &[
                "rev-parse".into(),
                "--git-path".into(),
                "objects/info/alternates".into(),
            ],
            None,
        )?;
        let text = std::str::from_utf8(&raw)
            .ok()
            .and_then(|text| text.strip_suffix('\n'))
            .filter(|text| !text.is_empty())
            .ok_or_else(|| SnapshotError::new(InternalError, "invalid Git object store path"))?;
        let path = self.root.join(text);
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Ok(_) => Err(SnapshotError::new(
                InternalError,
                "Git object alternates are unsupported; use a self-contained clone",
            )),
            Err(_) => Err(SnapshotError::new(
                InternalError,
                "cannot inspect Git object alternates",
            )),
        }
    }
}

use crate::{
    domain::{RepoPath, TreeOid},
    service::ports::git::{GitSnapshotPort, RetainedTree, SnapshotFile},
};
impl GitSnapshotPort for GitCliAdapter {
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.create_tree(files)
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
