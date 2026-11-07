use super::{
    GitRepository,
    constants::{BLOB_KIND, OBJECT_INTEGRITY_MESSAGE, TREE_KIND},
    durability::{StagedObject, object_reuse_existing},
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
    git.reader
        .borrow_mut()
        .tree_bytes_get_by_oid(oid, missing, wrong)
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
    if let Some(oid) = object_reuse_existing(&git.repo, kind, bytes, &git.cleanup)? {
        return Ok(oid.to_string());
    }
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

#[cfg(test)]
mod tests {
    use super::tree_get_by_oid;
    use crate::{
        domain::TreeOid,
        git::{GitRepository, validation::tree_validation_get_count},
        process_test_support::{FORMATS, TempRepo},
        service::{GitSnapshots, SnapshotError, SnapshotErrorKind},
    };
    use std::fs;

    #[test]
    fn tree_reads_reuse_structure_only_within_the_owned_reader() {
        for format in FORMATS {
            // Given: an independently written Git tree and a fresh product reader.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let expected = fixture.run_git_successfully(&["cat-file", "tree", &oid], None);
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let before = tree_validation_get_count();
            // When: the same immutable body is demanded twice by this reader.
            assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            // Then: structural work is shared, but a separate owner starts unvalidated.
            assert_eq!(tree_validation_get_count() - before, 1);
            let separate = GitRepository::open(fixture.get_root(), &[]).unwrap();
            assert_eq!(get_tree(&separate, &oid).unwrap(), expected);
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn memoized_tree_reads_reject_replacements_and_recover_after_restoration() {
        for format in FORMATS {
            // Given: a previously validated tree and independently written replacement objects.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let empty = fixture.get_oid(&["hash-object", "-w", "-t", "tree", "--stdin"], Some(b""));
            let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"CANARY"));
            let original = fs::read(fixture.build_object_path(&oid)).unwrap();
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let expected = get_tree(&git, &oid).unwrap();
            for (replacement, kind) in [
                (Some(&empty), SnapshotErrorKind::InternalError),
                (Some(&blob), SnapshotErrorKind::InvalidRequest),
                (None, SnapshotErrorKind::NotFound),
            ] {
                // When: the object's current backing bytes change or disappear after memoization.
                if let Some(replacement) = replacement {
                    fixture.replace_loose_object(&oid, replacement);
                } else {
                    fs::remove_file(fixture.build_object_path(&oid)).unwrap();
                }
                let error = get_tree(&git, &oid).unwrap_err();
                // Then: current failures remain visible; restoring valid bytes permits recovery.
                assert_eq!(error.get_kind(), kind);
                assert!(!format!("{error:?}").contains("CANARY"));
                if replacement.is_some() {
                    fs::remove_file(fixture.build_object_path(&oid)).unwrap();
                }
                fs::write(fixture.build_object_path(&oid), &original).unwrap();
                assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            }
        }
    }

    #[test]
    fn invalid_tree_bodies_are_validated_on_every_demand() {
        for format in FORMATS {
            // Given: a correctly hashed tree with a forbidden zero-prefixed mode.
            let fixture = TempRepo::new(format);
            let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"source"));
            let mut bytes = b"0100644 file\0".to_vec();
            bytes.extend_from_slice(
                gix::hash::ObjectId::from_hex(blob.as_bytes())
                    .unwrap()
                    .as_bytes(),
            );
            let oid = fixture.get_oid(
                &["hash-object", "-w", "--literally", "-t", "tree", "--stdin"],
                Some(&bytes),
            );
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let before = tree_validation_get_count();
            // When: the malformed body is demanded twice from the same owner.
            let first = get_tree(&git, &oid).unwrap_err();
            let second = get_tree(&git, &oid).unwrap_err();
            // Then: failures never authorize a later structural-validation bypass.
            assert_eq!(first.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(second.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn full_audits_validate_tree_structure_despite_addressed_read_memoization() {
        for format in FORMATS {
            // Given: the root body has already passed an addressed read on this owner.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let tree = TreeOid::parse(&oid).unwrap();
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            get_tree(&git, &oid).unwrap();
            let before = tree_validation_get_count();
            // When: two independent full closure audits retain that root.
            git.tree_pin_by_oid(&tree).unwrap();
            git.tree_pin_by_oid(&tree).unwrap();
            // Then: each full audit rechecks its tree body, independent of the memo.
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn tree_validation_memo_overflow_revalidates_without_rejecting_valid_objects() {
        for format in FORMATS {
            // Given: 4097 independently written distinct valid trees exceed the metadata budget.
            let fixture = TempRepo::new(format);
            let trees = create_trees(&fixture, 4097);
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            for oid in &trees {
                get_tree(&git, oid).unwrap();
            }
            let before = tree_validation_get_count();
            // When: the last cached and first uncached bodies are each demanded twice.
            for _ in 0..2 {
                get_tree(&git, &trees[4095]).unwrap();
                get_tree(&git, &trees[4096]).unwrap();
            }
            // Then: overflow remains accepted but is structurally checked every time.
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    fn get_tree(git: &GitRepository, oid: &str) -> Result<Vec<u8>, SnapshotError> {
        tree_get_by_oid(
            git,
            oid,
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
    }

    fn create_trees(fixture: &TempRepo, count: usize) -> Vec<String> {
        let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"source"));
        let blob = gix::hash::ObjectId::from_hex(blob.as_bytes()).unwrap();
        let mut paths = String::new();
        for index in 0..count {
            let path = fixture.get_root().join(format!(".git/memo-input-{index}"));
            let mut bytes = format!("100644 file-{index}\0").into_bytes();
            bytes.extend_from_slice(blob.as_bytes());
            fs::write(&path, bytes).unwrap();
            paths.push_str(path.to_str().unwrap());
            paths.push('\n');
        }
        let output = fixture.run_git_successfully(
            &["hash-object", "-w", "-t", "tree", "--stdin-paths"],
            Some(paths.as_bytes()),
        );
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}
