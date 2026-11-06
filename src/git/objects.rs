use super::{
    GitRepository,
    constants::{BLOB_KIND, OBJECT_INTEGRITY_MESSAGE, TREE_KIND},
    durability::StagedObject,
    tree::tree_entry_get,
    validation::tree_bytes_validate,
};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{SnapshotEntry, SnapshotError, SnapshotErrorKind, SnapshotFile},
};
use gix::{hash::ObjectId, objs::Kind};
use std::collections::{BTreeMap, BTreeSet};

const FOREIGN_OID_FORMAT: &str = "OID does not match repository object format";
const MISSING_SNAPSHOT_PATH: &str = "snapshot path is missing";
const GITLINK_KIND: &str = "gitlink";

#[derive(Default)]
struct Directory<'a> {
    files: BTreeMap<&'a str, BlobOid>,
    dirs: BTreeMap<&'a str, Directory<'a>>,
}

/// Creates a tree, validating all paths first and writing exact blobs.
///
/// # Arguments
///
/// * `git` — repository whose excluded roots and object store are used.
/// * `files` — complete snapshot files whose exact bytes and paths form the tree.
pub(super) fn tree_create(
    git: &GitRepository,
    files: &[SnapshotFile<'_>],
) -> Result<TreeOid, SnapshotError> {
    paths_validate(
        git.excluded_roots,
        &files.iter().map(|file| file.path).collect::<Vec<_>>(),
    )?;
    let mut root = Directory::default();
    for file in files {
        root.entry_insert_by_path(file.path.as_str(), blob_write(git, file.bytes)?);
    }
    root.tree_write(git)
}

/// Writes exact bytes durably without filters or newline normalization.
///
/// # Arguments
///
/// * `git` — repository whose object store receives the blob.
/// * `bytes` — exact payload written without filters or newline normalization.
pub(super) fn blob_write(git: &GitRepository, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
    BlobOid::parse(&object_write(git, Kind::Blob, bytes)?)
        .map_err(|_| SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE))
}

/// Creates a tree, validating all paths and verifying each distinct untrusted blob first.
///
/// # Arguments
///
/// * `git` — repository whose excluded roots, reader and object store are used.
/// * `entries` — snapshot entries whose declared paths and existing blobs form the tree.
pub(super) fn tree_assemble(
    git: &GitRepository,
    entries: &[SnapshotEntry],
) -> Result<TreeOid, SnapshotError> {
    paths_validate(
        git.excluded_roots,
        &entries.iter().map(|entry| &entry.path).collect::<Vec<_>>(),
    )?;
    let mut verified = BTreeSet::new();
    for entry in entries {
        if entry.oid.as_str().len() != git.oid_len {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                FOREIGN_OID_FORMAT,
            ));
        }
        if verified.insert(entry.oid.as_str()) {
            git.reader.borrow_mut().object_get_by_oid(
                entry.oid.as_str(),
                BLOB_KIND,
                SnapshotErrorKind::InvalidRequest,
                SnapshotErrorKind::InvalidRequest,
            )?;
        }
    }
    let mut root = Directory::default();
    for entry in entries {
        root.entry_insert_by_path(entry.path.as_str(), entry.oid.clone());
    }
    root.tree_write(git)
}

/// Returns one blob's bytes after verifying the root and addressed objects without auditing
/// unrelated descendants.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — root tree identity whose addressed path is read.
/// * `path` — repository-relative path of the blob within the tree.
pub(super) fn blob_get_by_tree_oid_and_path(
    git: &GitRepository,
    oid: &TreeOid,
    path: &RepoPath,
) -> Result<Vec<u8>, SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    let mut current = oid.as_str().to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        let bytes = tree_get_by_oid(
            git,
            &current,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        let (kind, oid) = tree_entry_get_by_name(git, &bytes, part.as_bytes())?;
        if parts.peek().is_none() {
            if kind != BLOB_KIND {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "snapshot path is not a blob",
                ));
            }
            return git.reader.borrow_mut().object_bytes_get_by_oid(
                &oid,
                BLOB_KIND,
                SnapshotErrorKind::InternalError,
                SnapshotErrorKind::InternalError,
            );
        }
        if kind != TREE_KIND {
            return Err(SnapshotError::new(
                SnapshotErrorKind::NotFound,
                MISSING_SNAPSHOT_PATH,
            ));
        }
        current = oid;
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::NotFound,
        MISSING_SNAPSHOT_PATH,
    ))
}

/// Verifies the root tree's hash, type and own structure without claiming closure integrity.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — tree identity whose hash, type and own structure are checked.
pub(super) fn tree_validate_by_oid(
    git: &GitRepository,
    oid: &TreeOid,
) -> Result<(), SnapshotError> {
    if oid.as_str().len() != git.oid_len {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InvalidRequest,
            FOREIGN_OID_FORMAT,
        ));
    }
    tree_get_by_oid(
        git,
        oid.as_str(),
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    )
    .map(|_| ())
}

/// Returns one verified tree body without traversing its children.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — hexadecimal object identity of the tree to read.
/// * `missing` — error kind used when the object is absent.
/// * `wrong` — error kind used when the object is not a tree.
pub(super) fn tree_get_by_oid(
    git: &GitRepository,
    oid: &str,
    missing: SnapshotErrorKind,
    wrong: SnapshotErrorKind,
) -> Result<Vec<u8>, SnapshotError> {
    let bytes = git
        .reader
        .borrow_mut()
        .object_bytes_get_by_oid(oid, TREE_KIND, missing, wrong)?;
    tree_bytes_validate(&bytes, git.oid_len)?;
    Ok(bytes)
}

fn tree_entry_get_by_name(
    git: &GitRepository,
    mut bytes: &[u8],
    name: &[u8],
) -> Result<(&'static str, String), SnapshotError> {
    while !bytes.is_empty() {
        let (entry, rest) = tree_entry_get(bytes, git.oid_len)?;
        if entry.name == name {
            let kind = match entry.mode & 0o170000 {
                0o040000 => TREE_KIND,
                0o160000 => GITLINK_KIND,
                _ => BLOB_KIND,
            };
            let oid = ObjectId::try_from(entry.oid)
                .map_err(|_| {
                    SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
                })?
                .to_string();
            return Ok((kind, oid));
        }
        bytes = rest;
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::NotFound,
        MISSING_SNAPSHOT_PATH,
    ))
}

impl<'a> Directory<'a> {
    fn entry_insert_by_path(&mut self, path: &'a str, oid: BlobOid) {
        match path.split_once('/') {
            Some((dir, rest)) => self
                .dirs
                .entry(dir)
                .or_default()
                .entry_insert_by_path(rest, oid),
            None => {
                self.files.insert(path, oid);
            }
        }
    }

    fn tree_write(&self, git: &GitRepository) -> Result<TreeOid, SnapshotError> {
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
                ("40000", *name, dir.tree_write(git)?.as_str().to_owned()),
            );
        }
        let mut bytes = Vec::new();
        for (mode, name, oid) in entries.values() {
            bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            let oid = ObjectId::from_hex(oid.as_bytes()).map_err(|_| {
                SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
            })?;
            bytes.extend_from_slice(oid.as_bytes());
        }
        tree_bytes_validate(&bytes, git.oid_len)?;
        TreeOid::parse(&object_write(git, Kind::Tree, &bytes)?).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
        })
    }
}

fn object_write(git: &GitRepository, kind: Kind, bytes: &[u8]) -> Result<String, SnapshotError> {
    let object = StagedObject::new_with_cleanup(&git.repo, kind, bytes, &git.cleanup)?;
    let oid = object.object_get_oid().to_string();
    object.object_publish()?;
    Ok(oid)
}

fn paths_validate(excluded_roots: &[&str], paths: &[&RepoPath]) -> Result<(), SnapshotError> {
    let mut unique = BTreeSet::new();
    for path in paths {
        let path = path.as_str();
        let mut parts = path.split('/');
        if parts.clone().any(|part| part.eq_ignore_ascii_case(".git"))
            || parts
                .next()
                .is_some_and(|root| excluded_roots.contains(&root))
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
