use super::{
    GitCli,
    constants::{
        BLOB_KIND, COMMIT_KIND, EXECUTABLE_FILE_MODE, GITLINK_KIND, GITLINK_MODE,
        REGULAR_FILE_MODE, SYMLINK_MODE, TREE_KIND, TREE_MODE,
    },
};
use crate::{
    domain::{RepoPath, TreeOid},
    service::{SnapshotError, SnapshotErrorKind, SnapshotFile},
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
fn make_integrity_error() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git object integrity failed",
    )
}
impl ObjectDb<'_> {
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
    fn parse_oid(&self, raw: &[u8]) -> Result<TreeOid, SnapshotError> {
        let text = std::str::from_utf8(raw)
            .map_err(|_| make_integrity_error())?
            .trim_end_matches('\n');
        if text.len() != self.git.oid_len {
            return Err(make_integrity_error());
        }
        TreeOid::parse(text).map_err(|_| make_integrity_error())
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
        let actual = self.parse_oid(&self.run(
            &["hash-object", "-t", kind, "--no-filters", "--stdin"],
            Some(&bytes),
        )?)?;
        if actual.as_str() != oid {
            return Err(make_integrity_error());
        }
        Ok(bytes)
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
            self.parse_oid(parts[2].as_bytes())?;
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
    pub(super) fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        let mut paths = BTreeSet::new();
        // Validate the entire request before the first object write.
        for file in files {
            let path = file.path.as_str();
            let mut parts = path.split('/');
            if parts.clone().any(|p| p.eq_ignore_ascii_case(".git"))
                || parts.next() == Some(".letopis")
                || !paths.insert(path)
            {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "invalid snapshot paths",
                ));
            }
        }
        for path in &paths {
            for (i, c) in path.char_indices() {
                if c == '/' && paths.contains(&path[..i]) {
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InvalidRequest,
                        "snapshot file and directory collide",
                    ));
                }
            }
        }
        let mut root = Directory::default();
        for file in files {
            let oid = self.parse_oid(&self.run(
                &["hash-object", "-w", "--no-filters", "--stdin"],
                Some(file.bytes),
            )?)?;
            root.insert(file.path.as_str(), oid);
        }
        root.write(self)
    }
}
#[derive(Default)]
struct Directory<'a> {
    files: BTreeMap<&'a str, TreeOid>,
    dirs: BTreeMap<&'a str, Directory<'a>>,
}
impl<'a> Directory<'a> {
    fn insert(&mut self, path: &'a str, oid: TreeOid) {
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
        db.parse_oid(&db.run(&["mktree", "-z"], Some(&input))?)
    }
}
