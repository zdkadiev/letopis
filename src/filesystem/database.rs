use crate::{
    service::{StorageError, StorageErrorKind, StorageResult},
    utils::has_same_identity,
};
use rustix::{
    fd::{AsFd, BorrowedFd},
    fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, open, statat},
    io::Errno,
};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// File identities retained across one database open or attachment.
#[derive(Debug)]
pub(crate) struct DatabasePaths<'a> {
    path: &'a Path,
    pinned: BorrowedFd<'a>,
    cache: Option<Stat>,
    vectors: Option<Stat>,
}

impl<'a> DatabasePaths<'a> {
    /// Observe regular, distinct leaves under the retained service directory.
    ///
    /// # Errors
    /// Rejects replaced directories, symlinks, aliases and inaccessible paths.
    pub(crate) fn observe(path: &'a Path, pinned: &'a impl AsFd) -> StorageResult<Self> {
        let mut observation = Self {
            path,
            pinned: pinned.as_fd(),
            cache: None,
            vectors: None,
        };
        observation.verify()?;
        observation.cache = observation.get_regular_stat("cache.db")?;
        observation.vectors = observation.get_regular_stat("vectors.db")?;
        observation.verify()?;
        Ok(observation)
    }

    /// Return whether a supported database leaf existed when observed.
    ///
    /// # Errors
    /// Rejects names other than the two application databases.
    pub(crate) fn get_database_exists(&self, name: &str) -> StorageResult<bool> {
        match name {
            "cache.db" => Ok(self.cache.is_some()),
            "vectors.db" => Ok(self.vectors.is_some()),
            _ => Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "unsupported storage database",
            )),
        }
    }

    /// Verify original leaves remain unchanged and newly created leaves are safe.
    ///
    /// # Errors
    /// Rejects replacement, deletion, nonregular files and database aliases.
    pub(crate) fn verify(&self) -> StorageResult<()> {
        let directory = open(
            self.path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(map_path_error)?;
        let current = fstat(&directory).map_err(map_path_error)?;
        let pinned = fstat(self.pinned).map_err(map_path_error)?;
        if !has_same_identity(&current, &pinned) {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "document storage directory changed",
            ));
        }
        let cache = self.get_regular_stat("cache.db")?;
        let vectors = self.get_regular_stat("vectors.db")?;
        for (before, after) in [(&self.cache, &cache), (&self.vectors, &vectors)] {
            if let Some(before) = before
                && !after
                    .as_ref()
                    .is_some_and(|after| has_same_identity(before, after))
            {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidRequest,
                    "document storage database changed",
                ));
            }
        }
        if let (Some(cache), Some(vectors)) = (cache, vectors)
            && has_same_identity(&cache, &vectors)
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "storage databases must be distinct files",
            ));
        }
        Ok(())
    }

    fn get_regular_stat(&self, name: &str) -> StorageResult<Option<Stat>> {
        match statat(self.pinned, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile => {
                Ok(Some(stat))
            }
            Ok(_) => Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "unsafe document storage path",
            )),
            Err(Errno::NOENT) => Ok(None),
            Err(error) => Err(map_path_error(error)),
        }
    }
}

fn map_path_error(error: Errno) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InvalidRequest,
        "cannot validate document storage path",
        error,
    )
}

/// Resolve distinct database paths for fixture and author-supplied storage.
///
/// # Errors
/// Rejects invalid paths, inaccessible parents and aliased databases.
pub(crate) fn prepare_paths(cache: &Path, vectors: &Path) -> StorageResult<(PathBuf, PathBuf)> {
    let cache = resolve_path(cache)?;
    let vectors = resolve_path(vectors)?;
    if cache == vectors || has_same_file(&cache, &vectors)? {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "storage databases must be distinct files",
        ));
    }
    Ok((cache, vectors))
}

fn resolve_path(path: &Path) -> StorageResult<PathBuf> {
    let raw = path
        .to_str()
        .filter(|raw| !raw.is_empty() && !raw.contains('\0'))
        .ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            )
        })?;
    let path = Path::new(raw);
    let resolved = if path.exists() {
        fs::canonicalize(path)
    } else {
        let name = path.file_name().ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            )
        })?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::canonicalize(parent).map(|parent| parent.join(name))
    }
    .map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "storage parent or file is unavailable",
            error,
        )
    })?;
    if resolved.to_str().is_none()
        || resolved.is_dir()
        || (!path.exists() && fs::symlink_metadata(path).is_ok())
    {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage database path",
        ));
    }
    Ok(resolved)
}

fn has_same_file(cache: &Path, vectors: &Path) -> StorageResult<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let read = |path| match fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StorageError::new_with_cause(
                StorageErrorKind::InvalidRequest,
                "storage file identity unavailable",
                error,
            )),
        };
        if let (Some(cache), Some(vectors)) = (read(cache)?, read(vectors)?) {
            return Ok(cache.dev() == vectors.dev() && cache.ino() == vectors.ino());
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{DatabasePaths, prepare_paths, resolve_path};
    use crate::service::StorageErrorKind;
    use std::fs::{self, File};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn rejects_replacement_of_an_observed_database() {
        // Given: a retained directory and an existing database leaf.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        fs::write(path.join("cache.db"), b"original").unwrap();
        let observation = DatabasePaths::observe(&path, &pinned).unwrap();
        // When: the original leaf is replaced while the directory remains unchanged.
        fs::rename(path.join("cache.db"), path.join("saved.db")).unwrap();
        fs::write(path.join("cache.db"), b"replacement").unwrap();
        // Then: post-operation verification detects the changed file identity.
        assert!(observation.verify().is_err());
        assert_eq!(fs::read(path.join("saved.db")).unwrap(), b"original");
    }

    #[test]
    fn permits_creation_but_rejects_hard_link_aliases() {
        // Given: an observation before either writable database exists.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        let observation = DatabasePaths::observe(&path, &pinned).unwrap();
        // When: distinct regular leaves are created.
        fs::write(path.join("cache.db"), []).unwrap();
        fs::write(path.join("vectors.db"), []).unwrap();
        // Then: creation is valid, but replacing vectors with a cache alias is rejected.
        observation.verify().unwrap();
        fs::remove_file(path.join("vectors.db")).unwrap();
        fs::hard_link(path.join("cache.db"), path.join("vectors.db")).unwrap();
        assert!(observation.verify().is_err());
    }

    #[test]
    fn rejects_database_paths_with_nul_bytes() {
        // Given / When: resolve a path containing an embedded NUL byte.
        let error = resolve_path(Path::new("bad\0database")).unwrap_err();
        // Then: SQLite must never receive a path that is not a filesystem name.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }

    #[test]
    fn rejects_an_existing_directory_as_a_database_path() {
        // Given: a directory exists where a database file is expected.
        let root = TempDir::new().unwrap();
        // When: resolve the directory as a database path.
        let error = resolve_path(root.path()).unwrap_err();
        // Then: reject the invalid request without changing the filesystem.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }

    #[test]
    fn preserves_non_absence_errors_when_observing_database_identity() {
        // Given: a regular parent causes ENOTDIR rather than a missing leaf.
        let root = TempDir::new().unwrap();
        let parent = root.path().join("ordinary-file");
        fs::write(&parent, b"preserved").unwrap();
        // When: resolve database leaves below the regular file.
        let error =
            prepare_paths(&parent.join("cache.db"), &root.path().join("vectors.db")).unwrap_err();
        // Then: reject the invalid request without changing the filesystem.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(fs::read(&parent).unwrap(), b"preserved");
        assert!(!root.path().join("vectors.db").exists());
    }
}
