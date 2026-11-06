mod database_tests {
    use crate::service::StorageResult;
    use crate::storage::open_database;
    use rusqlite::Connection;
    use rustix::fd::{AsFd, BorrowedFd};
    use std::{cell::RefCell, path::Path};
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
    };
    use tempfile::TempDir;

    #[test]
    fn reject_cache_attachment_symlink_installed_after_validation() {
        // Given: pin the service directory and open the vector connection.
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("vectors.db").unwrap();
        let link = path.join("cache.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        // When / Then: the read-only ATTACH inherits NOFOLLOW, including a URI.
        assert!(directory.attach_cache(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
        assert!(!outside.path().join("outside.db-wal").exists());
    }

    #[test]
    fn reject_database_leaf_symlink_installed_after_directory_validation() {
        for name in ["cache.db", "vectors.db"] {
            // Given
            let root = TempDir::new().unwrap();
            let path = fs::canonicalize(root.path()).unwrap();
            let outside = TempDir::new().unwrap();
            let target = outside.path().join("outside.db");
            fs::write(&target, []).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = DatabaseFixture::new(&path, &pinned);
            let link = path.join(name);
            let destination = target.clone();
            directory.before_open.replace(Some(Box::new(move || {
                symlink(destination, link).unwrap();
            })));
            // When / Then
            assert!(directory.open_database(name).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"");
        }
    }

    #[test]
    fn reject_vector_attachment_symlink_installed_after_validation() {
        // Given
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("cache.db").unwrap();
        let link = path.join("vectors.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        // When / Then
        assert!(directory.attach_vectors(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
    }

    #[test]
    fn reject_replaced_service_directory_before_initialization() {
        for use_symlink in [false, true] {
            // Given
            let root = TempDir::new().unwrap();
            let root_path = fs::canonicalize(root.path()).unwrap();
            let path = root_path.join(".letopis");
            fs::create_dir(&path).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = DatabaseFixture::new(&path, &pinned);
            fs::rename(&path, root_path.join("original")).unwrap();
            let outside = TempDir::new().unwrap();
            if use_symlink {
                symlink(outside.path(), &path).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            // When / Then
            assert!(directory.open_database("cache.db").is_err());
            assert!(!path.join("cache.db").exists());
            assert!(!outside.path().join("cache.db").exists());
        }
    }

    #[test]
    fn rejects_regular_cache_replacement_during_attachment() {
        // Given: an existing cache and vector connection under a pinned directory.
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let pinned = File::open(&path).unwrap();
        drop(open_database(&path.join("cache.db")).unwrap());
        drop(open_database(&path.join("replacement.db")).unwrap());
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("vectors.db").unwrap();
        let replaced = path.clone();
        directory.before_open.replace(Some(Box::new(move || {
            fs::rename(replaced.join("cache.db"), replaced.join("original.db")).unwrap();
            fs::rename(replaced.join("replacement.db"), replaced.join("cache.db")).unwrap();
        })));
        // When: the cache leaf changes during SQLite attachment.
        let result = directory.attach_cache(&connection);
        // Then: a regular replacement is rejected even though directory identity matches.
        assert!(result.is_err());
    }

    struct DatabaseFixture<'a> {
        path: &'a Path,
        pinned: BorrowedFd<'a>,
        before_open: RefCell<Option<Box<dyn FnOnce()>>>,
    }

    impl<'a> DatabaseFixture<'a> {
        fn new(path: &'a Path, pinned: &'a impl AsFd) -> Self {
            Self {
                path,
                pinned: pinned.as_fd(),
                before_open: RefCell::new(None),
            }
        }

        fn open_database(&self, name: &str) -> StorageResult<Connection> {
            super::super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::super::open_confined_database(self.path, &self.pinned, name)
        }

        fn attach_vectors(&self, connection: &Connection) -> StorageResult<()> {
            super::super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::super::attach_confined_vectors(self.path, &self.pinned, connection)
        }

        fn attach_cache(&self, connection: &Connection) -> StorageResult<()> {
            super::super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::super::attach_confined_cache(self.path, &self.pinned, connection)
        }
    }

    mod storage_contracts {
        use crate::service::TransactionError;
        use crate::storage::get_current;

        use crate::storage::TestDatabases;
        use crate::{
            domain::Sha256,
            service::{CacheDatabase, StorageError, StorageRead, VectorDatabase},
        };
        use std::fs::File;
        use tempfile::TempDir;

        #[test]
        fn nested_read_works_on_both_production_open_paths() {
            // Given: each production opener constructs the shared cache storage.
            for confined in [false, true] {
                let root = TempDir::new().unwrap();
                let path = root.path().canonicalize().unwrap();
                let pinned = File::open(&path).unwrap();
                let storage = if confined {
                    crate::storage::open_confined_test_storage(&path, &pinned).unwrap()
                } else {
                    crate::storage::open_test_storage(
                        &path.join("cache.db"),
                        &path.join("vectors.db"),
                    )
                    .unwrap()
                };
                // When: a service joins a read scope inside the cache write scope.
                let result =
                    storage
                        .cache_storage
                        .with_write_tx(|| -> Result<u64, StorageError> {
                            storage
                                .cache_storage
                                .with_read_tx(|| storage.cache_storage.project_get_by_id())
                                .map_err(|error| match error {
                                    TransactionError::Storage(error)
                                    | TransactionError::Operation(error) => error,
                                })
                        });
                // Then: the nested read uses the real writer and sees its starting revision.
                assert_eq!(result.unwrap(), 0);
                assert!(
                    storage
                        .cache_storage
                        .get_test_writer()
                        .execute("delete from vec.meta", [])
                        .is_err()
                );
            }
        }

        #[test]
        fn vector_writer_wait_is_bounded_without_changing_cache_or_read_policy() {
            for confined in [false, true] {
                // Given: exercise both supported open paths against independent databases.
                let root = TempDir::new().unwrap();
                let path = root.path().canonicalize().unwrap();
                let pinned = File::open(&path).unwrap();
                let storage = if confined {
                    crate::storage::open_confined_test_storage(&path, &pinned).unwrap()
                } else {
                    crate::storage::open_test_storage(
                        &path.join("cache.db"),
                        &path.join("vectors.db"),
                    )
                    .unwrap()
                };
                // When / Then: only vectors writers have the five-second contention budget.
                for (connection, expected) in [
                    (storage.vector_storage.get_test_connection(), 5000),
                    (storage.cache_storage.get_test_writer(), 0),
                    (storage.cache_storage.get_test_reader(), 0),
                ] {
                    let timeout: i64 = connection
                        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
                        .unwrap();
                    assert_eq!(timeout, expected);
                }
            }
        }

        #[test]
        fn confined_vector_pruning_uses_read_only_cache_and_closes_cleanly() {
            // Given: only the confined production opener is under test.
            let root = TempDir::new().unwrap();
            let path = root.path().canonicalize().unwrap();
            let pinned = File::open(&path).unwrap();
            let storage = crate::storage::open_confined_test_storage(&path, &pinned).unwrap();
            let identity = Sha256::digest(b"confined");
            seed_vector(&storage, &identity);
            // When: pruning requires the read-only cache attachment used for hash-reference checks.
            storage
                .vector_storage
                .with_write_tx(&identity, || {
                    let unit = &storage.vector_storage;
                    unit.prune_unreferenced().unwrap();

                    Ok::<_, StorageError>(())
                })
                .map(get_current)
                .unwrap();
            // Then: both native deletion and lifetime/permission policy.
            assert_eq!(
                get_current(
                    storage
                        .cache_storage
                        .with_read_tx(|| storage
                            .cache_storage
                            .vector_keys_get_by_search_hash(&identity, &identity))
                        .unwrap()
                ),
                None
            );
            assert!(
                storage
                    .vector_storage
                    .get_test_connection()
                    .execute_batch("delete from cache.nodes")
                    .is_err()
            );
            drop(storage);
            assert!(!path.join("cache.db-wal").exists());
            assert!(!path.join("cache.db-shm").exists());
        }

        fn seed_vector(storage: &TestDatabases, identity: &Sha256) {
            storage
                .vector_storage
                .reset_vectors(None, identity)
                .unwrap();
            storage
                .vector_storage
                .with_write_tx(identity, || {
                    storage
                        .vector_storage
                        .store_vector(identity, &[1.0, 0.0, 0.0], 3)
                })
                .map(get_current)
                .unwrap();
        }
    }
}

mod database_path_tests {
    use super::super::{
        StorageResult, resolve_database_paths, wait_for_stable_storage_paths,
        with_verified_database_paths,
    };
    use crate::service::{FileError, FileErrorKind, StorageError, StorageErrorKind};
    use std::fs::{self, File};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn retries_transient_changed_database_observations() {
        // Given: a path verification that reports a concurrent creation as changed once.
        let attempts = std::cell::Cell::new(0);
        // When: storage verification observes the same leaves again.
        let result = wait_for_stable_storage_paths(|| {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err(StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "document storage database changed",
                    FileError::new(FileErrorKind::Changed),
                ))
            } else {
                Ok(())
            }
        });
        // Then: the transient change is retried and the stable observation succeeds.
        result.unwrap();
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn rejects_replacement_of_an_observed_database() {
        // Given: a retained directory and a regular database leaf.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        fs::write(path.join("cache.db"), b"original").unwrap();
        // When: a database operation replaces the original leaf and fails.
        let error = with_verified_database_paths(&path, &pinned, |_, _| {
            fs::rename(path.join("cache.db"), path.join("saved.db")).unwrap();
            fs::write(path.join("cache.db"), b"replacement").unwrap();
            Err::<(), _>(super::super::StorageError::new(
                StorageErrorKind::InternalError,
                "sqlite failed",
            ))
        })
        .unwrap_err();
        // Then: integrity verification takes precedence over the database failure.
        assert_eq!(error.get_message(), "document storage database changed");
        assert_eq!(fs::read(path.join("saved.db")).unwrap(), b"original");
    }

    #[test]
    fn rejects_directory_replacement_before_fresh_file_observation() {
        // Given: the original directory capability survives path replacement.
        let root = TempDir::new().unwrap();
        let path = root.path().join("state");
        fs::create_dir(&path).unwrap();
        let pinned = File::open(&path).unwrap();
        fs::rename(&path, root.path().join("saved")).unwrap();
        fs::create_dir(&path).unwrap();
        // When: composition starts a new observation under the replaced name.
        let error = with_verified_database_paths(&path, &pinned, |_, _| -> StorageResult<()> {
            panic!("unsafe operation entered")
        })
        .unwrap_err();
        // Then: the retained original capability rejects the fresh directory.
        assert_eq!(error.get_message(), "document storage directory changed");
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    }

    #[test]
    fn rejects_existing_database_alias_before_entering_operation() {
        // Given: both database names already refer to one regular file.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        fs::write(path.join("cache.db"), b"untouched").unwrap();
        fs::hard_link(path.join("cache.db"), path.join("vectors.db")).unwrap();
        // When: composition validates the pair before giving it to SQLite.
        let error = with_verified_database_paths(&path, &pinned, |_, _| -> StorageResult<()> {
            panic!("aliased operation entered")
        })
        .unwrap_err();
        // Then: alias detection prevents the operation and preserves the original bytes.
        assert_eq!(
            error.get_message(),
            "storage databases must be distinct files"
        );
        assert_eq!(fs::read(path.join("cache.db")).unwrap(), b"untouched");
    }

    #[test]
    fn permits_creation_but_rejects_hard_link_aliases() {
        // Given: both files are initially absent beneath the pinned directory.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        // When: the operation creates independent regular files.
        with_verified_database_paths(&path, &pinned, |_, _| {
            fs::write(path.join("cache.db"), []).unwrap();
            fs::write(path.join("vectors.db"), []).unwrap();
            Ok(())
        })
        .unwrap();
        fs::remove_file(path.join("vectors.db")).unwrap();
        // When: an operation fills the missing leaf with an alias to the cache.
        let error = with_verified_database_paths(&path, &pinned, |_, _| {
            fs::hard_link(path.join("cache.db"), path.join("vectors.db")).unwrap();
            Ok(())
        })
        .unwrap_err();
        // Then: post-operation distinctness rejects the alias.
        assert_eq!(
            error.get_message(),
            "storage databases must be distinct files"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_non_utf8_targets_resolved_from_utf8_symlinks() {
        use std::ffi::OsString;
        use std::os::unix::{ffi::OsStringExt, fs::symlink};
        // Given: a UTF-8 alias resolves to a native path SQLite cannot represent.
        let root = TempDir::new().unwrap();
        let target = root.path().join(OsString::from_vec(vec![0xff]));
        fs::write(&target, b"untouched").unwrap();
        let alias = root.path().join("alias");
        symlink(&target, &alias).unwrap();
        // When: composition resolves external storage paths.
        let error = resolve_database_paths(&alias, &root.path().join("other")).unwrap_err();
        // Then: the original safe classification and context are preserved.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(error.get_message(), "invalid storage database path");
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        assert!(!root.path().join("other").exists());
    }

    #[test]
    fn rejects_database_paths_with_nul_bytes() {
        // Given / When: resolve a path containing an embedded NUL byte.
        let error =
            resolve_database_paths(Path::new("bad\0database"), Path::new("other")).unwrap_err();
        // Then: SQLite must never receive a path that is not a filesystem name.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }

    #[test]
    fn rejects_an_existing_directory_as_a_database_path() {
        // Given: a directory exists where a database file is expected.
        let root = TempDir::new().unwrap();
        // When: resolve the directory as a database path.
        let error = resolve_database_paths(root.path(), &root.path().join("other")).unwrap_err();
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
            resolve_database_paths(&parent.join("cache.db"), &root.path().join("vectors.db"))
                .unwrap_err();
        // Then: reject the invalid request without changing the filesystem.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(fs::read(&parent).unwrap(), b"preserved");
        assert!(!root.path().join("vectors.db").exists());
    }
}
