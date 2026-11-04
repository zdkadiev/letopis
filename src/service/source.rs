use super::{
    CacheDatabase, DocumentError, DocumentResult, FileSystem, NodeMetadata, SourceAvailability,
    SourceObservation, SourceReader,
};
use crate::domain::{EdgeTarget, Sha256, SourceUri};

#[derive(Debug)]
pub(crate) struct SourceService<F, C> {
    filesystem: F,
    storage: std::rc::Rc<C>,
}

impl<F: FileSystem, C> SourceService<F, C> {
    pub(crate) fn new(storage: std::rc::Rc<C>, filesystem: F) -> Self {
        Self {
            filesystem,
            storage,
        }
    }
}

impl<F: FileSystem, C: CacheDatabase> SourceReader for SourceService<F, C> {
    fn get_source(&self, uri: &SourceUri) -> DocumentResult<Option<SourceObservation>> {
        self.storage
            .with_read_tx(|| {
                let Some(row) = self
                    .storage
                    .nodes_get_by_id(&EdgeTarget::Source(uri.clone()))?
                else {
                    return Ok(None);
                };
                let NodeMetadata::Source(mut metadata) = row.metadata else {
                    return Ok(None);
                };
                let Some(local) = uri.get_local() else {
                    metadata.hash = None;
                    metadata.lines = None;
                    return Ok(Some(SourceObservation {
                        metadata,
                        availability: SourceAvailability::Unfetched,
                    }));
                };
                let file = self.filesystem.get_file(local.get_path())?;
                metadata.hash = file
                    .as_ref()
                    .and_then(|file| local.select_bytes(&file.bytes))
                    .map(Sha256::digest);
                metadata.lines = local.get_lines();
                let availability = if metadata.hash.is_some() {
                    SourceAvailability::Available
                } else {
                    SourceAvailability::SourceNotFound
                };
                Ok(Some(SourceObservation {
                    metadata,
                    availability,
                }))
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => DocumentError::from(error),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        CapturedFile, DocumentError, DocumentErrorKind, DocumentRefreshService, DocumentResult,
        FileStat, FileSystem, ProvenanceService, Refresher, SourceAvailability, SourceReader,
    };
    use super::SourceService;
    use crate::{
        domain::{RepoPath, SourceUri},
        filesystem::LocalFileSystem,
        git::GitRepository,
        markdown::MarkdownParser,
        storage::TestDatabases,
    };
    use std::{cell::RefCell, error::Error, fs, io, path::Path, process::Command, rc::Rc};

    #[test]
    fn publish_remote_without_file_capture_and_group_local_ranges() {
        // Given: local range bindings and a remote source share a recorded filesystem.
        let repo = create_repo();
        fs::write(repo.path().join("source.txt"), b"one\ntwo\n").unwrap();
        fs::write(repo.path().join("a.md"), "<!-- letopis: {\"created_at\":3} -->\n[source](source.txt#L1-L1 \"letopis:type=external;path=source.txt#L1-L1\") [other](source.txt#L2-L2 \"letopis:type=external;path=source.txt#L2-L2\") [remote](https://Host/a?x=1#section \"letopis:type=external;path=https://Host/a?x=1#section\")\n").unwrap();
        let recorded_reads = Rc::new(RefCell::new(Vec::new()));
        let filesystem = RecordedFiles::new(repo.path(), recorded_reads.clone());
        let storage = open_storage(repo.path());
        let provenance = ProvenanceService::new(
            Box::new(GitRepository::open(repo.path()).unwrap()),
            RecordedFiles::new(repo.path(), recorded_reads.clone()),
        );
        let mut refresher = DocumentRefreshService {
            git: GitRepository::open(repo.path()).unwrap(),
            filesystem,
            markdown: MarkdownParser::new(),
            storage: std::rc::Rc::new(storage.cache),
            provenance,
        };

        // When: document refresh publishes the source projection through boxed Git provenance.
        refresher.refresh().unwrap();

        let reads = recorded_reads.borrow();
        // Then: local ranges share their final capture and the remote binding causes no file I/O.
        assert_eq!(
            reads
                .iter()
                .filter(|path| path.as_str() == "source.txt")
                .count(),
            // Preflight observes both local bindings; projection groups their final capture.
            3
        );
        assert!(
            reads
                .iter()
                .all(|path| matches!(path.as_str(), "a.md" | "source.txt")),
            "unexpected source I/O: {reads:?}"
        );
    }

    #[test]
    fn skip_io_for_unknown_or_knowledge_address() {
        // Given: unknown and knowledge addresses with recorded source reads.
        let repo = prepare_sources();
        let recorded_reads = Rc::new(RefCell::new(Vec::new()));
        let reader = SourceService::new(
            Rc::new(open_storage(repo.path()).cache),
            RecordedFiles::new(repo.path(), recorded_reads.clone()),
        );
        let unknown = SourceUri::parse("unknown.txt").unwrap();
        let knowledge = SourceUri::parse("a.md").unwrap();

        // When / Then: the source reader resolves those addresses; no source filesystem read occurs.
        assert!(reader.get_source(&unknown).unwrap().is_none());
        assert!(reader.get_source(&knowledge).unwrap().is_none());
        // Then: no source filesystem read occurs.
        assert!(recorded_reads.borrow().is_empty());
    }

    #[test]
    fn return_unfetched_remote_without_io() {
        // Given: a declared remote source with recorded reads.
        let repo = prepare_sources();
        let recorded_reads = Rc::new(RefCell::new(Vec::new()));
        let reader = SourceService::new(
            Rc::new(open_storage(repo.path()).cache),
            RecordedFiles::new(repo.path(), recorded_reads.clone()),
        );
        let remote = SourceUri::parse("https://Host/a?x=1#section").unwrap();

        // When: the remote source is observed.
        let observed = reader.get_source(&remote).unwrap().unwrap();

        // Then: it remains unfetched without local I/O.
        assert_eq!(observed.availability, SourceAvailability::Unfetched);
        assert_eq!(observed.metadata.hash, None);
        assert_eq!(observed.metadata.lines, None);
        assert!(recorded_reads.borrow().is_empty());
    }

    #[test]
    fn observe_live_source_without_referrer_edits() {
        // Given: a declared local source and unchanged referrers.
        let repo = prepare_sources();
        let recorded_reads = Rc::new(RefCell::new(Vec::new()));
        let reader = SourceService::new(
            Rc::new(open_storage(repo.path()).cache),
            RecordedFiles::new(repo.path(), recorded_reads.clone()),
        );
        let uri = SourceUri::parse("source.txt").unwrap();
        fs::write(repo.path().join("source.txt"), b"def").unwrap();

        // When: the live source bytes are observed.
        let observed = reader.get_source(&uri).unwrap().unwrap();

        // Then: current bytes are returned without editing referrers.
        assert_eq!(observed.availability, SourceAvailability::Available);
        assert_eq!(
            observed.metadata.hash.unwrap().to_hex(),
            "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
        );
        assert_eq!(
            *recorded_reads.borrow(),
            [RepoPath::parse("source.txt").unwrap()]
        );
    }

    #[test]
    fn propagate_private_source_failure() {
        // Given: a source read that fails with a private cause.
        let repo = prepare_sources();
        let recorded_reads = Rc::new(RefCell::new(Vec::new()));
        let mut files = RecordedFiles::new(repo.path(), recorded_reads);
        files.failure = Some(RepoPath::parse("source.txt").unwrap());
        let reader = SourceService::new(Rc::new(open_storage(repo.path()).cache), files);
        let uri = SourceUri::parse("source.txt").unwrap();

        // When: the live source is requested.
        let error = reader.get_source(&uri).unwrap_err();

        // Then: a safe typed error preserves the private cause internally.
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert!(!error.to_string().contains("PRIVATE_SOURCE_CAUSE"));
        assert!(!format!("{error:?}").contains("PRIVATE_SOURCE_CAUSE"));
        assert_eq!(error.source().unwrap().to_string(), "PRIVATE_SOURCE_CAUSE");
    }

    struct RecordedFiles {
        inner: LocalFileSystem,
        reads: Rc<RefCell<Vec<RepoPath>>>,
        failure: Option<RepoPath>,
    }

    impl RecordedFiles {
        fn new(root: &Path, reads: Rc<RefCell<Vec<RepoPath>>>) -> Self {
            Self {
                inner: LocalFileSystem::open(root).unwrap(),
                reads,
                failure: None,
            }
        }
    }

    impl FileSystem for RecordedFiles {
        type IndexLock = <LocalFileSystem as FileSystem>::IndexLock;

        fn get_file_stat_by_path(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
            self.inner.get_file_stat_by_path(path)
        }

        fn get_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
            self.reads.borrow_mut().push(path.clone());
            if self.failure.as_ref() == Some(path) {
                return Err(DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "cannot read source",
                    io::Error::other("PRIVATE_SOURCE_CAUSE"),
                ));
            }
            self.inner.get_file(path)
        }

        fn get_time_ns(&self) -> DocumentResult<i64> {
            self.inner.get_time_ns()
        }

        fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
            self.inner.lock_index()
        }
    }

    fn prepare_sources() -> tempfile::TempDir {
        let repo = create_repo();
        fs::write(repo.path().join("source.txt"), b"abc").unwrap();
        fs::write(repo.path().join("a.md"), "<!-- letopis: {\"created_at\":3} -->\n[local](source.txt \"letopis:type=external;path=source.txt\") [remote](https://Host/a?x=1#section \"letopis:type=external;path=https://Host/a?x=1#section\")\n").unwrap();
        crate::wire_document_refresher(repo.path())
            .unwrap()
            .refresh()
            .unwrap();
        repo
    }

    fn create_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        repo
    }

    fn open_storage(root: &Path) -> TestDatabases {
        fs::create_dir_all(root.join(".letopis")).unwrap();
        crate::storage::open_test_storage(
            &root.join(".letopis/cache.db"),
            &root.join(".letopis/vectors.db"),
        )
        .unwrap()
    }
}
