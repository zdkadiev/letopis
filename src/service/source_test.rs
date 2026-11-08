use crate::service::{
    FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult, MockFileReader,
};

use super::super::{
    CapturedFile, DocumentErrorKind, DocumentRefreshService, FileStat, ProvenanceService,
    Refresher, SourceAvailability, SourceReader,
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
        Box::new(GitRepository::open(repo.path(), &[crate::utils::SERVICE_DIRECTORY]).unwrap()),
        RecordedFiles::new(repo.path(), recorded_reads.clone()),
    );
    let mut refresher = DocumentRefreshService {
        clock: Box::new(crate::process::SystemClock::new()),
        git: GitRepository::open(repo.path(), &[crate::utils::SERVICE_DIRECTORY]).unwrap(),
        filesystem,
        markdown: MarkdownParser::new(),
        cache: std::rc::Rc::new(storage.cache_storage),
        provenance,
    };

    // When: document refresh publishes the source projection through boxed Git provenance.
    refresher.refresh(&[]).unwrap();

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
        Rc::new(open_storage(repo.path()).cache_storage),
        RecordedFiles::new(repo.path(), recorded_reads.clone()),
    );
    let unknown = SourceUri::parse("unknown.txt").unwrap();
    let knowledge = SourceUri::parse("a.md").unwrap();

    // When / Then: the source reader resolves those addresses; no source filesystem read occurs.
    assert!(reader.get_source_by_uri(&unknown).unwrap().is_none());
    assert!(reader.get_source_by_uri(&knowledge).unwrap().is_none());
    // Then: no source filesystem read occurs.
    assert!(recorded_reads.borrow().is_empty());
}

#[test]
fn return_unfetched_remote_without_io() {
    // Given: a declared remote source with recorded reads.
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let reader = SourceService::new(
        Rc::new(open_storage(repo.path()).cache_storage),
        RecordedFiles::new(repo.path(), recorded_reads.clone()),
    );
    let remote = SourceUri::parse("https://Host/a?x=1#section").unwrap();

    // When: the remote source is observed.
    let observed = reader.get_source_by_uri(&remote).unwrap().unwrap();

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
        Rc::new(open_storage(repo.path()).cache_storage),
        RecordedFiles::new(repo.path(), recorded_reads.clone()),
    );
    let uri = SourceUri::parse("source.txt").unwrap();
    fs::write(repo.path().join("source.txt"), b"def").unwrap();

    // When: the live source bytes are observed.
    let observed = reader.get_source_by_uri(&uri).unwrap().unwrap();

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
fn excludes_only_stable_nonregular_source_leaves() {
    for (kind, excluded) in [
        (FileErrorKind::NonRegular, true),
        (FileErrorKind::UnsafePath, false),
        (FileErrorKind::Changed, false),
        (FileErrorKind::Io, false),
    ] {
        // Given: a recorded source and a boundary reporting one precise file outcome.
        let repo = prepare_sources();
        let mut files = MockFileReader::new();
        files
            .expect_file_get_by_path()
            .withf(|path| path == std::path::Path::new("source.txt"))
            .times(1)
            .returning(move |_| Err(FileError::new(kind)));
        let reader = SourceService::new(Rc::new(open_storage(repo.path()).cache_storage), files);
        // When: the source consumer interprets the file outcome.
        let result = reader.get_source_by_uri(&SourceUri::parse("source.txt").unwrap());
        // Then: only a stable nonregular final entry is missing; all failures stay errors.
        if excluded {
            assert_eq!(
                result.unwrap().unwrap().availability,
                SourceAvailability::SourceNotFound
            );
        } else {
            assert_eq!(
                result.unwrap_err().get_kind(),
                DocumentErrorKind::InternalError
            );
        }
    }
}

#[test]
fn propagate_private_source_failure() {
    // Given: a source read that fails with a private cause.
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let mut files = RecordedFiles::new(repo.path(), recorded_reads);
    files.failure = Some(RepoPath::parse("source.txt").unwrap());
    let reader = SourceService::new(Rc::new(open_storage(repo.path()).cache_storage), files);
    let uri = SourceUri::parse("source.txt").unwrap();

    // When: the live source is requested.
    let error = reader.get_source_by_uri(&uri).unwrap_err();

    // Then: a safe typed error preserves the private cause internally.
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!error.to_string().contains("PRIVATE_SOURCE_CAUSE"));
    assert!(!format!("{error:?}").contains("PRIVATE_SOURCE_CAUSE"));
    assert_eq!(
        error.source().unwrap().source().unwrap().to_string(),
        "PRIVATE_SOURCE_CAUSE"
    );
}

struct RecordedFiles {
    inner: LocalFileSystem,
    reads: Rc<RefCell<Vec<RepoPath>>>,
    failure: Option<RepoPath>,
}

impl RecordedFiles {
    fn new(root: &Path, reads: Rc<RefCell<Vec<RepoPath>>>) -> Self {
        Self {
            inner: LocalFileSystem::new(root).unwrap(),
            reads,
            failure: None,
        }
    }
}

impl FileReader for RecordedFiles {
    fn symlink_is_exist(&self, path: &Path) -> FileResult<bool> {
        self.inner.symlink_is_exist(path)
    }

    fn file_get_stat_by_path(&self, path: &std::path::Path) -> FileResult<Option<FileStat>> {
        self.inner.file_get_stat_by_path(path)
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        self.inner.file_get_bytes_by_path(path)
    }

    fn file_get_by_path(&self, path: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        self.reads
            .borrow_mut()
            .push(RepoPath::parse(path.to_str().unwrap()).unwrap());
        if self
            .failure
            .as_ref()
            .is_some_and(|failure| failure.as_str() == path.to_str().unwrap())
        {
            return Err(FileError::new_with_cause(
                FileErrorKind::Io,
                io::Error::other("PRIVATE_SOURCE_CAUSE"),
            ));
        }
        self.inner.file_get_by_path(path)
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unexpected file_get_identity_by_path")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unexpected file_is_exist")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_identity_by_path")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_distinct_by_paths")
    }

    fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<std::os::fd::OwnedFd>> {
        self.inner.directory_get_by_path(path)
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        panic!("unexpected lock_get_owner_by_path")
    }
}

impl FileLocker for RecordedFiles {
    type Lock = <LocalFileSystem as FileLocker>::Lock;

    fn lock_acquire_by_path(&self, path: &std::path::Path) -> FileResult<Self::Lock> {
        self.inner.lock_acquire_by_path(path)
    }

    fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
        panic!("unexpected lock probe")
    }
}

fn prepare_sources() -> tempfile::TempDir {
    let repo = create_repo();
    fs::write(repo.path().join("source.txt"), b"abc").unwrap();
    fs::write(repo.path().join("a.md"), "<!-- letopis: {\"created_at\":3} -->\n[local](source.txt \"letopis:type=external;path=source.txt\") [remote](https://Host/a?x=1#section \"letopis:type=external;path=https://Host/a?x=1#section\")\n").unwrap();
    crate::wire_document_refresher(repo.path())
        .unwrap()
        .refresh(&[])
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
