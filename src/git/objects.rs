use super::{
    GitRepository,
    constants::{BLOB_KIND, GITLINK_KIND, TREE_KIND},
    durability::StagedObject,
    tree::read_entry,
    validation::validate_tree_bytes,
};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{SnapshotEntry, SnapshotError, SnapshotErrorKind, SnapshotFile},
    utils::SERVICE_DIRECTORY,
};
use gix::{hash::ObjectId, objs::Kind};
use std::collections::{BTreeMap, BTreeSet};
const FOREIGN_OID_FORMAT: &str = "OID does not match repository object format";
const MISSING_SNAPSHOT_PATH: &str = "snapshot path is missing";

/// Raw snapshot assembly and addressed reads using one repository's controlled reader.
#[derive(Debug)]
pub(super) struct ObjectDb<'a> {
    pub(super) git: &'a GitRepository,
}
#[derive(Default)]
struct Directory<'a> {
    files: BTreeMap<&'a str, BlobOid>,
    dirs: BTreeMap<&'a str, Directory<'a>>,
}
impl ObjectDb<'_> {
    /// Validate the entire path request before writing exact blobs and ordered trees.
    pub(super) fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        validate_paths(&files.iter().map(|file| file.path).collect::<Vec<_>>())?;
        let mut root = Directory::default();
        for file in files {
            root.insert(file.path.as_str(), self.write_blob(file.bytes)?);
        }
        root.write(self)
    }
    /// Durably publish exact bytes without filters or newline normalization.
    pub(super) fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        BlobOid::parse(&write_object(self.git, Kind::Blob, bytes)?)
            .map_err(|_| make_integrity_error())
    }
    /// Preflight all paths and verify every distinct untrusted blob before publishing trees.
    pub(super) fn create_tree_from_blobs(
        &self,
        entries: &[SnapshotEntry],
    ) -> Result<TreeOid, SnapshotError> {
        validate_paths(&entries.iter().map(|entry| &entry.path).collect::<Vec<_>>())?;
        let mut verified = BTreeSet::new();
        for entry in entries {
            if entry.oid.as_str().len() != self.git.oid_len {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    FOREIGN_OID_FORMAT,
                ));
            }
            if verified.insert(entry.oid.as_str()) {
                self.git.reader.borrow_mut().read_object(
                    entry.oid.as_str(),
                    BLOB_KIND,
                    SnapshotErrorKind::InvalidRequest,
                    SnapshotErrorKind::InvalidRequest,
                )?;
            }
        }
        let mut root = Directory::default();
        for entry in entries {
            root.insert(entry.path.as_str(), entry.oid.clone());
        }
        root.write(self)
    }
    /// Verify the root and addressed objects, without auditing unrelated descendants.
    pub(super) fn read_blob(
        &self,
        oid: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.validate_tree(oid)?;
        let mut current = oid.as_str().to_owned();
        let mut parts = path.as_str().split('/').peekable();
        while let Some(part) = parts.next() {
            let bytes = self.read_tree(
                &current,
                SnapshotErrorKind::InternalError,
                SnapshotErrorKind::InternalError,
            )?;
            let (kind, oid) = self.find_entry(&bytes, part.as_bytes())?;
            if parts.peek().is_none() {
                if kind != BLOB_KIND {
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InvalidRequest,
                        "snapshot path is not a blob",
                    ));
                }
                return self.git.reader.borrow_mut().read_bytes(
                    &oid,
                    BLOB_KIND,
                    SnapshotErrorKind::InternalError,
                    SnapshotErrorKind::InternalError,
                );
            }
            if kind != TREE_KIND {
                return Err(missing_path());
            }
            current = oid;
        }
        Err(missing_path())
    }
    /// Check the root's hash, exact type and structure without claiming closure integrity.
    pub(super) fn validate_tree(&self, oid: &TreeOid) -> Result<(), SnapshotError> {
        if oid.as_str().len() != self.git.oid_len {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                FOREIGN_OID_FORMAT,
            ));
        }
        self.read_tree(
            oid.as_str(),
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .map(|_| ())
    }
    fn read_tree(
        &self,
        oid: &str,
        missing: SnapshotErrorKind,
        wrong: SnapshotErrorKind,
    ) -> Result<Vec<u8>, SnapshotError> {
        let bytes = self
            .git
            .reader
            .borrow_mut()
            .read_bytes(oid, TREE_KIND, missing, wrong)?;
        validate_tree_bytes(&bytes, self.git.oid_len)?;
        Ok(bytes)
    }
    fn find_entry(
        &self,
        mut bytes: &[u8],
        name: &[u8],
    ) -> Result<(&'static str, String), SnapshotError> {
        while !bytes.is_empty() {
            let (entry, rest) = read_entry(bytes, self.git.oid_len)?;
            if entry.name == name {
                let kind = match entry.mode & 0o170000 {
                    0o040000 => TREE_KIND,
                    0o160000 => GITLINK_KIND,
                    _ => BLOB_KIND,
                };
                let oid = ObjectId::try_from(entry.oid)
                    .map_err(|_| make_integrity_error())?
                    .to_string();
                return Ok((kind, oid));
            }
            bytes = rest;
        }
        Err(missing_path())
    }
}
impl<'a> Directory<'a> {
    fn insert(&mut self, path: &'a str, oid: BlobOid) {
        match path.split_once('/') {
            Some((dir, rest)) => self.dirs.entry(dir).or_default().insert(rest, oid),
            None => {
                self.files.insert(path, oid);
            }
        }
    }
    fn write(&self, db: &ObjectDb<'_>) -> Result<TreeOid, SnapshotError> {
        // Git compares directory names with a virtual '/' terminator.
        let mut entries = BTreeMap::new();
        for (name, oid) in &self.files {
            entries.insert(
                (*name).to_owned(),
                ("100644", *name, oid.as_str().to_owned()),
            );
        }
        for (name, dir) in &self.dirs {
            entries.insert(
                format!("{name}/"),
                ("40000", *name, dir.write(db)?.as_str().to_owned()),
            );
        }
        let mut bytes = Vec::new();
        for (mode, name, oid) in entries.values() {
            bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            let oid = ObjectId::from_hex(oid.as_bytes()).map_err(|_| make_integrity_error())?;
            bytes.extend_from_slice(oid.as_bytes());
        }
        validate_tree_bytes(&bytes, db.git.oid_len)?;
        TreeOid::parse(&write_object(db.git, Kind::Tree, &bytes)?)
            .map_err(|_| make_integrity_error())
    }
}
fn write_object(git: &GitRepository, kind: Kind, bytes: &[u8]) -> Result<String, SnapshotError> {
    let object = StagedObject::new_with_cleanup(&git.repo, kind, bytes, &git.cleanup)?;
    let oid = object.get_oid().to_string();
    object.publish()?;
    Ok(oid)
}

fn missing_path() -> SnapshotError {
    SnapshotError::new(SnapshotErrorKind::NotFound, MISSING_SNAPSHOT_PATH)
}
fn validate_paths(paths: &[&RepoPath]) -> Result<(), SnapshotError> {
    let mut unique = BTreeSet::new();
    for path in paths {
        let path = path.as_str();
        let mut parts = path.split('/');
        if parts.clone().any(|part| part.eq_ignore_ascii_case(".git"))
            || parts.next() == Some(SERVICE_DIRECTORY)
            || !unique.insert(path)
        {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "invalid snapshot paths",
            ));
        }
    }
    for path in &unique {
        for (offset, character) in path.char_indices() {
            if character == '/' && unique.contains(&path[..offset]) {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "snapshot file and directory collide",
                ));
            }
        }
    }
    Ok(())
}

fn make_integrity_error() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git object integrity failed",
    )
}
