use super::super::{
    CapturedFile, DocumentError, DocumentErrorKind, DocumentRefresher, DocumentResult, FileStat,
    FileSystem, ProvenanceService, Refresher, SourceAvailability, SourceReader, Storage,
};
use super::SourceService;
use crate::{
    domain::{RepoPath, SourceUri},
    filesystem::LocalFileSystem,
    git::GitCli,
    markdown::MarkdownParser,
    storage::SqliteStorage,
};
use std::{cell::RefCell, error::Error, fs, io, path::Path, process::Command, rc::Rc};

#[test]
fn publish_remote_without_file_capture_and_group_local_ranges() {
    // Given
    let repo = create_repo();
    fs::write(repo.path().join("source.txt"), b"one\ntwo\n").unwrap();
    fs::write(repo.path().join("a.md"), "<!-- letopis: {\"created_at\":3} -->\n[source](source.txt#L1-L1 \"letopis:type=external\") [other](source.txt#L2-L2 \"letopis:type=external\") [remote](https://Host/a?x=1#section \"letopis:type=external\")\n").unwrap();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let filesystem = RecordedFiles::new(repo.path(), recorded_reads.clone());
    let storage = open_storage(repo.path());
    let provenance = ProvenanceService::new(
        GitCli::open(repo.path()).unwrap(),
        RecordedFiles::new(repo.path(), recorded_reads.clone()),
    );
    let mut refresher = DocumentRefresher {
        git: GitCli::open(repo.path()).unwrap(),
        filesystem,
        markdown: MarkdownParser::new(),
        storage,
        provenance,
    };

    // When
    refresher.refresh().unwrap();

    let reads = recorded_reads.borrow();
    // Then
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
    // Given
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let reader = SourceService::new(RecordedFiles::new(repo.path(), recorded_reads.clone()));
    let mut storage = open_storage(repo.path());
    let mut view = storage.begin_read().unwrap();
    let unknown = SourceUri::parse("unknown.txt").unwrap();
    let knowledge = SourceUri::parse("a.md").unwrap();

    // When / Then
    assert!(reader.read_source(&mut view, &unknown).unwrap().is_none());
    assert!(reader.read_source(&mut view, &knowledge).unwrap().is_none());
    // Then
    assert!(recorded_reads.borrow().is_empty());
}

#[test]
fn return_unfetched_remote_without_io() {
    // Given
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let reader = SourceService::new(RecordedFiles::new(repo.path(), recorded_reads.clone()));
    let mut storage = open_storage(repo.path());
    let mut view = storage.begin_read().unwrap();
    let remote = SourceUri::parse("https://Host/a?x=1#section").unwrap();

    // When
    let observed = reader.read_source(&mut view, &remote).unwrap().unwrap();

    // Then
    assert_eq!(observed.availability, SourceAvailability::Unfetched);
    assert_eq!(observed.metadata.hash, None);
    assert_eq!(observed.metadata.lines, None);
    assert!(recorded_reads.borrow().is_empty());
}

#[test]
fn observe_live_source_without_referrer_edits() {
    // Given
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let reader = SourceService::new(RecordedFiles::new(repo.path(), recorded_reads.clone()));
    let mut storage = open_storage(repo.path());
    let mut view = storage.begin_read().unwrap();
    let uri = SourceUri::parse("source.txt").unwrap();
    fs::write(repo.path().join("source.txt"), b"def").unwrap();

    // When
    let observed = reader.read_source(&mut view, &uri).unwrap().unwrap();

    // Then
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
    // Given
    let repo = prepare_sources();
    let recorded_reads = Rc::new(RefCell::new(Vec::new()));
    let mut files = RecordedFiles::new(repo.path(), recorded_reads);
    files.failure = Some(RepoPath::parse("source.txt").unwrap());
    let reader = SourceService::new(files);
    let mut storage = open_storage(repo.path());
    let mut view = storage.begin_read().unwrap();
    let uri = SourceUri::parse("source.txt").unwrap();

    // When
    let error = reader.read_source(&mut view, &uri).unwrap_err();

    // Then
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

    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
        self.inner.stat_file(path)
    }

    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        self.reads.borrow_mut().push(path.clone());
        if self.failure.as_ref() == Some(path) {
            return Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot read source",
                io::Error::other("PRIVATE_SOURCE_CAUSE"),
            ));
        }
        self.inner.read_file(path)
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
    fs::write(repo.path().join("a.md"), "<!-- letopis: {\"created_at\":3} -->\n[local](source.txt \"letopis:type=external\") [remote](https://Host/a?x=1#section \"letopis:type=external\")\n").unwrap();
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

fn open_storage(root: &Path) -> SqliteStorage {
    fs::create_dir_all(root.join(".letopis")).unwrap();
    SqliteStorage::open(
        &root.join(".letopis/cache.db"),
        &root.join(".letopis/vectors.db"),
    )
    .unwrap()
}
