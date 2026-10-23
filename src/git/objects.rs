use super::{
    GitCli,
    constants::{
        BLOB_KIND, COMMIT_KIND, EXECUTABLE_FILE_MODE, GITLINK_KIND, GITLINK_MODE,
        REGULAR_FILE_MODE, SYMLINK_MODE, TREE_KIND, TREE_MODE,
    },
};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{SnapshotEntry, SnapshotError, SnapshotErrorKind, SnapshotFile},
    utils::SERVICE_DIRECTORY,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
};

pub(super) struct ObjectDb<'a> {
    pub(super) git: &'a GitCli,
}
pub(super) struct Entry {
    pub(super) kind: String,
    pub(super) oid: String,
    pub(super) name: Vec<u8>,
}
#[derive(Default)]
struct Directory<'a> {
    files: BTreeMap<&'a str, BlobOid>,
    dirs: BTreeMap<&'a str, Directory<'a>>,
}
impl ObjectDb<'_> {
    pub(super) fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        // Preserve the all-request preflight before writing even the first blob.
        validate_paths(&files.iter().map(|file| file.path).collect::<Vec<_>>())?;
        let mut root = Directory::default();
        for file in files {
            root.insert(file.path.as_str(), self.write_blob(file.bytes)?);
        }
        root.write(self)
    }
    pub(super) fn create_tree_from_blobs(
        &self,
        entries: &[SnapshotEntry],
    ) -> Result<TreeOid, SnapshotError> {
        validate_paths(&entries.iter().map(|entry| &entry.path).collect::<Vec<_>>())?;
        for entry in entries {
            if entry.oid.as_str().len() != self.git.oid_len {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "OID does not match repository object format",
                ));
            }
            self.verify_object(
                entry.oid.as_str(),
                BLOB_KIND,
                SnapshotErrorKind::InvalidRequest,
                SnapshotErrorKind::InvalidRequest,
            )?;
        }
        let mut root = Directory::default();
        for entry in entries {
            root.insert(entry.path.as_str(), entry.oid.clone());
        }
        root.write(self)
    }
    pub(super) fn read_blob(
        &self,
        oid: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.validate_tree(oid)?;
        let mut current = oid.as_str().to_owned();
        let mut parts = path.as_str().split('/').peekable();
        while let Some(part) = parts.next() {
            let entry = self
                .read_entries(&current)?
                .into_iter()
                .find(|e| e.name == part.as_bytes())
                .ok_or_else(|| {
                    SnapshotError::new(SnapshotErrorKind::NotFound, "snapshot path is missing")
                })?;
            if parts.peek().is_none() {
                if entry.kind != BLOB_KIND {
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InvalidRequest,
                        "snapshot path is not a blob",
                    ));
                }
                return self.verify_object(
                    &entry.oid,
                    BLOB_KIND,
                    SnapshotErrorKind::InternalError,
                    SnapshotErrorKind::InternalError,
                );
            }
            if entry.kind != TREE_KIND {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::NotFound,
                    "snapshot path is missing",
                ));
            }
            current = entry.oid;
        }
        Err(SnapshotError::new(
            SnapshotErrorKind::NotFound,
            "snapshot path is missing",
        ))
    }
    pub(super) fn validate_tree(&self, oid: &TreeOid) -> Result<(), SnapshotError> {
        if oid.as_str().len() != self.git.oid_len {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "OID does not match repository object format",
            ));
        }
        self.verify_object(
            oid.as_str(),
            TREE_KIND,
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
        .map(|_| ())
    }
    pub(super) fn read_entries(&self, oid: &str) -> Result<Vec<Entry>, SnapshotError> {
        self.verify_object(
            oid,
            TREE_KIND,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        let raw = self.run(&["ls-tree", "-z", oid], None)?;
        let mut entries = Vec::new();
        for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = record
                .iter()
                .position(|b| *b == b'\t')
                .ok_or_else(make_integrity_error)?;
            let head = std::str::from_utf8(&record[..tab]).map_err(|_| make_integrity_error())?;
            let parts: Vec<_> = head.split(' ').collect();
            if parts.len() != 3 {
                return Err(make_integrity_error());
            }
            self.parse_oid_text(parts[2].as_bytes())?;
            let kind = match (parts[0], parts[1]) {
                (TREE_MODE, TREE_KIND) => TREE_KIND,
                (REGULAR_FILE_MODE | EXECUTABLE_FILE_MODE | SYMLINK_MODE, BLOB_KIND) => BLOB_KIND,
                (GITLINK_MODE, COMMIT_KIND) => GITLINK_KIND,
                _ => return Err(make_integrity_error()),
            };
            entries.push(Entry {
                kind: kind.into(),
                oid: parts[2].into(),
                name: record[tab + 1..].to_vec(),
            });
        }
        Ok(entries)
    }
    // Check type without peeling, read raw bytes and independently rehash without
    // writing. A decodable loose object under the wrong OID must never be trusted.
    pub(super) fn verify_object(
        &self,
        oid: &str,
        kind: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<Vec<u8>, SnapshotError> {
        let info = self.run(
            &["cat-file", "--batch-check"],
            Some(format!("{oid}\n").as_bytes()),
        )?;
        if info == format!("{oid} missing\n").as_bytes() {
            return Err(SnapshotError::new(missing, "Git object is missing"));
        }
        let info = std::str::from_utf8(&info).map_err(|_| make_integrity_error())?;
        let fields: Vec<_> = info.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != oid || fields[2].parse::<usize>().is_err() {
            return Err(make_integrity_error());
        }
        if fields[1] != kind {
            return Err(SnapshotError::new(wrong_type, "unexpected Git object type"));
        }
        let bytes = self.run(&["cat-file", kind, oid], None)?;
        if fields[2].parse::<usize>().ok() != Some(bytes.len()) {
            return Err(make_integrity_error());
        }
        let actual_raw = self.run(
            &["hash-object", "-t", kind, "--no-filters", "--stdin"],
            Some(&bytes),
        )?;
        let actual = self.parse_oid_text(&actual_raw)?;
        if actual != oid {
            return Err(make_integrity_error());
        }
        Ok(bytes)
    }
    pub(super) fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        let raw = self.run(
            &["hash-object", "-w", "--no-filters", "--stdin"],
            Some(bytes),
        )?;
        BlobOid::parse(self.parse_oid_text(&raw)?).map_err(|_| make_integrity_error())
    }
    pub(super) fn run(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.git.process.run(
            &self.git.root,
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            input,
        )
    }
    fn parse_tree_oid(&self, raw: &[u8]) -> Result<TreeOid, SnapshotError> {
        TreeOid::parse(self.parse_oid_text(raw)?).map_err(|_| make_integrity_error())
    }
    fn parse_oid_text<'a>(&self, raw: &'a [u8]) -> Result<&'a str, SnapshotError> {
        let text = std::str::from_utf8(raw)
            .map_err(|_| make_integrity_error())?
            .trim_end_matches('\n');
        if text.len() != self.git.oid_len
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(make_integrity_error());
        }
        Ok(text)
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
        let mut input = Vec::new();
        for (name, oid) in &self.files {
            input.extend_from_slice(
                format!("{REGULAR_FILE_MODE} {BLOB_KIND} {}\t{name}\0", oid.as_str()).as_bytes(),
            );
        }
        for (name, dir) in &self.dirs {
            let oid = dir.write(db)?;
            input.extend_from_slice(
                format!("{TREE_MODE} {TREE_KIND} {}\t{name}\0", oid.as_str()).as_bytes(),
            );
        }
        db.parse_tree_oid(&db.run(&["mktree", "-z"], Some(&input))?)
    }
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

#[cfg(test)]
mod tests {
    use super::{GitCli, ObjectDb};
    use crate::{git::process::GitProcess, service::SnapshotErrorKind};
    use std::path::PathBuf;

    #[test]
    fn object_output_requires_canonical_oid_in_the_repository_format() {
        // Arrange: output shape has no object-kind semantics.
        for length in [40, 64] {
            let git = GitCli {
                root: PathBuf::new(),
                process: GitProcess::new(),
                oid_len: length,
            };
            let db = ObjectDb { git: &git };
            let valid = "0123456789abcdef".repeat(4)[..length].to_owned();
            // Act / Assert
            assert_eq!(
                db.parse_oid_text(format!("{valid}\n").as_bytes()).unwrap(),
                valid
            );
            for raw in [
                "g".repeat(length),
                "A".repeat(length),
                "/".repeat(length),
                "a".repeat(length - 1),
                "a".repeat(length + 1),
            ] {
                assert_eq!(
                    db.parse_oid_text(raw.as_bytes()).unwrap_err().get_kind(),
                    SnapshotErrorKind::InternalError
                );
            }
            let mut invalid_utf8 = vec![b'a'; length];
            invalid_utf8[0] = 255;
            assert_eq!(
                db.parse_oid_text(&invalid_utf8).unwrap_err().get_kind(),
                SnapshotErrorKind::InternalError
            );
        }
    }
}
