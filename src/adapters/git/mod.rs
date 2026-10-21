mod objects;
mod process;
mod refs;
#[cfg(test)]
mod tests;
use crate::service::ports::git::{SnapshotError, SnapshotErrorKind::InternalError};
use process::{GitProcess, check_version};
use std::{
    ffi::OsString,
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
                .trim_end_matches('\n'),
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
        Ok(Self {
            root,
            process,
            oid_len,
        })
    }
}

use crate::{
    domain::{RepoPath, TreeOid},
    service::ports::git::{GitSnapshotPort, RetainedTree, SnapshotFile},
};
impl GitSnapshotPort for GitCliAdapter {
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        objects::ObjectDb { git: self }.create_tree(files)
    }
    fn read_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
        objects::ObjectDb { git: self }.read_blob(tree, path)
    }
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        refs::Retainer { git: self }.pin(tree)
    }
}
