#[cfg(test)]
mod tests;
use super::process::{GitProcess, check_version};
use super::{objects, refs};
use crate::{
    domain::{RepoPath, SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN, TreeOid},
    service::{
        CorpusFiles, DocumentError, DocumentErrorKind, DocumentResult, GitSnapshots, RetainedTree,
        SnapshotError, SnapshotErrorKind, SnapshotFile,
    },
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) struct GitCli {
    pub(super) root: PathBuf,
    pub(super) process: GitProcess,
    pub(super) oid_len: usize,
}
impl GitCli {
    pub(crate) fn get_root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn open(start: &Path) -> Result<Self, SnapshotError> {
        Self::open_with_process(start, GitProcess::new())
    }
    fn open_with_process(start: &Path, process: GitProcess) -> Result<Self, SnapshotError> {
        check_version(&process.run(start, &["--version".into()], None)?)?;
        let raw = process.run(start, &["rev-parse".into(), "--show-toplevel".into()], None)?;
        let root = PathBuf::from(
            String::from_utf8(raw)
                .map_err(|_| {
                    SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git root")
                })?
                .strip_suffix('\n')
                .ok_or_else(|| {
                    SnapshotError::new(
                        SnapshotErrorKind::InternalError,
                        "invalid Git root response",
                    )
                })?,
        );
        if !root.is_absolute() {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "invalid Git root",
            ));
        }
        let inside = process.run(
            &root,
            &["rev-parse".into(), "--is-inside-work-tree".into()],
            None,
        )?;
        if inside != b"true\n" {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "Git working copy required",
            ));
        }
        let format = process.run(
            &root,
            &[OsString::from("rev-parse"), "--show-object-format".into()],
            None,
        )?;
        let oid_len = match format.as_slice() {
            b"sha1\n" => SHA1_OID_HEX_LEN,
            b"sha256\n" => SHA256_OID_HEX_LEN,
            _ => {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
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
            .ok_or_else(|| {
                SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "invalid Git object store path",
                )
            })?;
        let path = self.root.join(text);
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
}

impl GitSnapshots for GitCli {
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

impl CorpusFiles for GitCli {
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        let raw = self
            .process
            .run(
                self.get_root(),
                &[
                    "ls-files".into(),
                    "-z".into(),
                    "--cached".into(),
                    "--others".into(),
                    "--exclude-standard".into(),
                ],
                None,
            )
            .map_err(|cause| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "cannot list document candidates",
                    cause,
                )
            })?;
        parse_candidates(&raw)
    }
}
fn parse_candidates(raw: &[u8]) -> DocumentResult<Vec<RepoPath>> {
    if !raw.is_empty() && !raw.ends_with(&[0]) {
        return Err(DocumentError::new(
            DocumentErrorKind::InternalError,
            "invalid Git candidate response",
        ));
    }
    let mut candidates = BTreeSet::new();
    for entry in raw
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        if !entry.ends_with(b".md") && !entry.ends_with(b".markdown") {
            continue;
        }
        if entry
            .split(|byte| *byte == b'/')
            .any(|part| part == b".git" || part == b".letopis")
        {
            continue;
        }
        let text = std::str::from_utf8(entry).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                "unsupported document path",
                cause,
            )
        })?;
        let path = RepoPath::parse(text).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                "unsupported document path",
                cause,
            )
        })?;
        candidates.insert(path);
    }
    Ok(candidates.into_iter().collect())
}
