use super::constants::REV_PARSE_COMMAND;
use super::process::{GitProcess, check_version, is_exit_code};
use super::{objects, refs};
use crate::{
    domain::{BlobOid, RepoPath, SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN, TreeOid},
    service::{
        CorpusLister, DocumentError, DocumentErrorKind, DocumentResult, GitSnapshots, RetainedTree,
        SnapshotEntry, SnapshotError, SnapshotErrorKind, SnapshotFile, SnapshotState,
        SnapshotVerifier,
    },
    utils::SERVICE_DIRECTORY,
};
use std::{
    collections::BTreeSet,
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
        let root = read_root(&process, start)?;
        require_work_tree(&process, &root)?;
        let oid_len = read_oid_len(&process, &root)?;
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
                REV_PARSE_COMMAND.into(),
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

impl SnapshotVerifier for GitCli {
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

impl CorpusLister for GitCli {
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
fn read_root(process: &GitProcess, start: &Path) -> Result<PathBuf, SnapshotError> {
    let raw = process
        .run(
            start,
            &[REV_PARSE_COMMAND.into(), "--show-toplevel".into()],
            None,
        )
        .map_err(|error| {
            if is_exit_code(&error, 128) {
                SnapshotError::new(SnapshotErrorKind::InvalidRequest, "Git repository required")
            } else {
                error
            }
        })?;
    let text = String::from_utf8(raw)
        .map_err(|_| SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git root"))?;
    let line = text.strip_suffix('\n').ok_or_else(|| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "invalid Git root response",
        )
    })?;
    let root = PathBuf::from(line);
    if !root.is_absolute() {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "invalid Git root",
        ));
    }
    Ok(root)
}

fn require_work_tree(process: &GitProcess, root: &Path) -> Result<(), SnapshotError> {
    let inside = process.run(
        root,
        &[REV_PARSE_COMMAND.into(), "--is-inside-work-tree".into()],
        None,
    )?;
    if inside != b"true\n" {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InvalidRequest,
            "Git working copy required",
        ));
    }
    Ok(())
}

fn read_oid_len(process: &GitProcess, root: &Path) -> Result<usize, SnapshotError> {
    let format = process.run(
        root,
        &[REV_PARSE_COMMAND.into(), "--show-object-format".into()],
        None,
    )?;
    match format.as_slice() {
        b"sha1\n" => Ok(SHA1_OID_HEX_LEN),
        b"sha256\n" => Ok(SHA256_OID_HEX_LEN),
        _ => Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "unsupported Git object format",
        )),
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
            .any(|part| part == b".git" || part == SERVICE_DIRECTORY.as_bytes())
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

#[cfg(test)]
mod tests;
