use super::{objects::ObjectDb, process::make_io_error};
use crate::service::{SnapshotError, SnapshotErrorKind};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Operation-owned tree metadata spool for one Git structural-validation process.
pub(super) struct TreeValidator {
    directory: PathBuf,
    paths: Vec<u8>,
    expected: Vec<u8>,
    count: usize,
}
impl TreeValidator {
    pub(super) fn new() -> Result<Self, SnapshotError> {
        let directory = create_directory()?;
        Ok(Self {
            directory,
            paths: Vec::new(),
            expected: Vec::new(),
            count: 0,
        })
    }
    pub(super) fn add_tree(&mut self, oid: &str, bytes: &[u8]) -> Result<(), SnapshotError> {
        let path = self.directory.join(self.count.to_string());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| make_io_error("cannot create Git validation input", e))?;
        file.write_all(bytes)
            .map_err(|e| make_io_error("cannot write Git validation input", e))?;
        // C-style octal quoting preserves arbitrary temporary-directory path bytes.
        self.paths.push(b'"');
        for byte in path.as_os_str().as_bytes() {
            self.paths
                .extend_from_slice(format!("\\{byte:03o}").as_bytes());
        }
        self.paths.extend_from_slice(b"\"\n");
        self.expected.extend_from_slice(oid.as_bytes());
        self.expected.push(b'\n');
        self.count += 1;
        Ok(())
    }
    pub(super) fn verify(self, db: &ObjectDb<'_>) -> Result<(), SnapshotError> {
        // hash-object includes Git's own strict tree-format checks, including
        // platform path aliases and special-file rules. Hashing alone cannot replace them.
        let actual = db.run(
            &["hash-object", "-t", "tree", "--no-filters", "--stdin-paths"],
            Some(&self.paths),
        )?;
        if actual != self.expected {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "Git tree validation identity mismatch",
            ));
        }
        fs::remove_dir_all(&self.directory)
            .map_err(|e| make_io_error("cannot remove Git validation inputs", e))
    }
}
impl Drop for TreeValidator {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn create_directory() -> Result<PathBuf, SnapshotError> {
    for _ in 0..16 {
        let path = std::env::temp_dir().join(format!(
            "letopis-trees-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(make_io_error("cannot create Git validation directory", e)),
        }
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "cannot allocate Git validation directory",
    ))
}

#[cfg(test)]
mod tests {
    use super::{ObjectDb, TreeValidator};
    use crate::{
        git::GitCli,
        process_test_support::{ObjectFormat, TempRepo},
    };
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn quotes_private_paths_and_cleans_up_successful_validation() {
        // Given: a private spool path containing spaces, quotes and a newline.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let git = GitCli::open(repo.get_root()).unwrap();
        let mut validator = TreeValidator::new().unwrap();
        let directory = validator.directory.with_extension("space \" quote\n");
        fs::rename(&validator.directory, &directory).unwrap();
        validator.directory = directory.clone();
        validator
            .add_tree("4b825dc642cb6eb9a060e54bf8d69288fbee4904", b"")
            .unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.join("0"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // When: Git reads C-quoted input paths through stdin-paths.
        validator.verify(&ObjectDb { git: &git }).unwrap();
        // Then: matching identities succeed and no owned metadata remains.
        assert!(!directory.exists());
    }

    #[test]
    fn rejects_changed_validation_input_and_cleans_up_failure() {
        // Given: the supplied identity does not match the valid tree bytes.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let git = GitCli::open(repo.get_root()).unwrap();
        let mut validator = TreeValidator::new().unwrap();
        let directory = validator.directory.clone();
        validator.add_tree(&"1".repeat(40), b"").unwrap();
        // When: independent Git verification observes the different identity.
        let result = validator.verify(&ObjectDb { git: &git });
        // Then: no success or leftover sensitive metadata.
        assert!(result.is_err());
        assert!(!directory.exists());
    }
}
