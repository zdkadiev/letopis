//! Production-process schedules. Only this private test driver reads control variables.
use super::super::DocumentRefreshService;
use crate::{
    domain::{Edge, RepoPath, Sha256},
    filesystem::LocalFileSystem,
    git::GitRepository,
    markdown::MarkdownParser,
    service::{
        CacheWrite, EmbeddingFailure, FileRow, FileSystem, GraphDiagnosticsService, NodeMetadata,
        ProvenanceService, QueryError, Refresher, StatusReader, StatusReport, StatusService,
        Storage, StorageError, StorageRead, StorageResult, StoredNode, VectorOutcome, VectorWrite,
    },
    storage::{SqliteStatusStorage, SqliteStorage},
    wire_document_refresher,
};
use rusqlite::Connection;
use std::{
    cell::RefCell,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::MetadataExt,
        net::{UnixListener, UnixStream},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const TIME: i64 = 42;
const WAIT: Duration = Duration::from_secs(20);
const CHILD: &str = "service::document_refresher::tests::process::run_child";
const SOURCE: &[u8] = b"external source\r\nexact bytes\n";
const OLD: &str = "<!-- letopis: {\"created_at\":1} -->\nAlphaToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
const NEW: &str = "<!-- letopis: {\"created_at\":3} -->\nBravoToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
const LATEST: &str = "<!-- letopis: {\"created_at\":3} -->\nCharlieToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
const DIAGNOSTIC_CANARY: &str = "TK28_DIAGNOSTIC_CANARY";
const INVALID: &str = "<!-- letopis: invalid TK28_DIAGNOSTIC_CANARY -->\n";
const RETAINED: &str = "<!-- letopis: {\"created_at\":2} -->\nRetainedToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
const CUTS: [&str; 10] = [
    "detected",
    "locked",
    "redetected",
    "begun",
    "cleared",
    "projections",
    "fts",
    "fingerprints",
    "before_commit",
    "after_commit",
];
type Service = DocumentRefreshService<
    GitRepository,
    LocalFileSystem,
    MarkdownParser,
    SqliteStorage,
    ProvenanceService<GitRepository, LocalFileSystem>,
>;
type Tokens = Vec<(String, i64, String, i64)>;
thread_local! {
    static CONTROL: RefCell<Option<UnixStream>> = const { RefCell::new(None) };
    static FAIL_LATE: RefCell<bool> = const { RefCell::new(false) };
}

pub(in crate::service) fn reach_checkpoint(name: &str) {
    CONTROL.with_borrow_mut(|control| {
        if let Some(stream) = control {
            writeln!(stream, "{name}").unwrap();
            eprintln!("checkpoint {name}");
            let mut response = [0];
            stream.read_exact(&mut response).unwrap();
            assert_eq!(response, [b'c']);
        }
    });
}

pub(in crate::service) fn fail_late<W: CacheWrite>(unit: &mut W) -> StorageResult<()> {
    if FAIL_LATE.with_borrow(|enabled| *enabled) {
        return unit.upsert_file(&FileRow {
            path: RepoPath::parse("invalid-size.md").unwrap(),
            size: u64::MAX,
            mtime_ns: 0,
            inode: 0,
            hash: Sha256::digest(b"overflow"),
            checked_at_ns: TIME,
            valid: true,
            error: None,
        });
    }
    Ok(())
}

#[test]
#[ignore = "subprocess entry; invoked only by the production schedule tests"]
fn run_child() {
    // Given
    let socket = std::env::var_os("TK10_SOCKET").expect("private child control");
    let root = PathBuf::from(std::env::var_os("TK10_ROOT").unwrap());
    let mode = std::env::var("TK10_MODE").unwrap();
    let stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    stream.set_write_timeout(Some(WAIT)).unwrap();
    CONTROL.with_borrow_mut(|control| *control = Some(stream));
    reach_checkpoint("connected");
    // When
    if let Some(mode) = mode.strip_prefix("normal-") {
        run_normal_service(&root, mode);
        reach_checkpoint("finished");
        return;
    }
    let mut service = build_service(&root);
    if mode == "hold" {
        let _guard = service.filesystem.lock_index().unwrap();
        reach_checkpoint("holding");
    } else {
        FAIL_LATE.with_borrow_mut(|enabled| *enabled = mode.ends_with("fail"));
        let result = if mode.starts_with("reindex") {
            service.reindex()
        } else {
            service.refresh()
        };
        eprintln!("refresh result: {result:?}");
        if mode.ends_with("fail") {
            let error = result.unwrap_err();
            // Then
            assert!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<StorageError>()
                    .is_some()
            );
            assert!(!format!("{error:?}").contains(DIAGNOSTIC_CANARY));
        } else {
            let result = result.unwrap();
            assert_eq!(result.published, mode != "noop");
        }
    }
    drop(service);
    reach_checkpoint("finished");
}

#[test]
fn normal_open_waits_at_index_lock_during_cache_write_then_redetects() {
    // Given: the owner holds a real cache transaction, not just index.lock.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let expected = fixture.build_expected(NEW, "BravoToken");
    let mut owner = Worker::spawn(fixture.root.path(), "publish");
    owner.reach("begun");
    // When: normal composition must open before the owner releases its SQLite writer.
    let mut waiter = Worker::spawn(fixture.root.path(), "normal-noop");
    waiter.reach("opened");
    waiter.resume();
    waiter.reach("contended");
    owner.resume();
    owner.finish();
    waiter.resume();
    waiter.finish();
    // Then: the waiter redetects the committed publication and does not republish.
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(NEW);
}

#[test]
fn normal_open_redetects_after_active_cache_writer_is_killed() {
    // Given a real transaction with an old publication still visible.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let mut owner = Worker::spawn(fixture.root.path(), "publish");
    owner.reach("begun");
    let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
    waiter.reach("contended");
    // When: change the file while waiting, then release the writer through SIGKILL.
    fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
    let expected = fixture.build_expected(LATEST, "CharlieToken");
    owner.kill();
    waiter.resume();
    waiter.finish();
    // Then: only the newest complete publication becomes visible.
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(LATEST);
}

#[test]
fn normal_open_and_noop_finish_during_vector_write_and_held_index_lock() {
    // Given independent live vector and kernel-lock owners.
    let fixture = Fixture::new();
    let mut owner = Worker::spawn(fixture.root.path(), "hold");
    owner.reach("holding");
    let mut storage = build_service(fixture.root.path()).storage;
    let identity = Sha256::digest(b"model identity");
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("identity changed");
    };
    unit.set_query_error(99, "uncommitted").unwrap();
    // When: normal composition and a no-op must finish with both owners still active.
    Worker::spawn(fixture.root.path(), "normal-noop").finish();
    // Then: neither the cache nor the committed vector state was changed.
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    assert_eq!(get_vectors(fixture.root.path()), fixture.vector_state);
    drop(unit);
    owner.kill();
}

struct Worker {
    child: Child,
    stream: BufReader<UnixStream>,
    _socket: TempDir,
    receipt: File,
}
impl Worker {
    fn spawn(root: &Path, mode: &str) -> Self {
        let socket = tempfile::Builder::new()
            .prefix("tk10-")
            .tempdir_in("/tmp")
            .unwrap();
        let listener = UnixListener::bind(socket.path().join("control")).unwrap();
        let evidence = std::env::var_os("LETOPIS_TEST_EVIDENCE")
            .map_or_else(|| PathBuf::from(".artifacts/tk10/process"), PathBuf::from);
        fs::create_dir_all(&evidence).unwrap();
        let name = socket.path().file_name().unwrap().to_str().unwrap();
        let log = File::create(evidence.join(format!("{name}.log"))).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD, "--ignored", "--nocapture"])
            .env("TK10_SOCKET", socket.path().join("control"))
            .env("TK10_ROOT", root)
            .env("TK10_MODE", mode)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(log)
            .spawn()
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let accept = thread::spawn(move || sender.send(listener.accept()).ok());
        let accepted = receiver.recv_timeout(WAIT);
        if accepted.is_err() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            // Unblock the accept thread before unwinding on a failed startup.
            let _ = UnixStream::connect(socket.path().join("control"));
            let _ = accept.join();
            panic!("child did not connect: {accepted:?}");
        }
        let (stream, _) = accepted.unwrap().unwrap();
        accept.join().unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let receipt = File::create(evidence.join(format!("{name}.receipt"))).unwrap();
        let mut worker = Self {
            child,
            stream: BufReader::new(stream),
            _socket: socket,
            receipt,
        };
        writeln!(worker.receipt, "mode={mode} pid={}", worker.child.id()).unwrap();
        assert_eq!(worker.get_event(), "connected");
        worker.resume();
        worker
    }
    fn get_event(&mut self) -> String {
        let mut line = String::new();
        assert!(
            self.stream.read_line(&mut line).unwrap() > 0,
            "child exited before acknowledgement"
        );
        writeln!(self.receipt, "{}", line.trim()).unwrap();
        line.trim().to_owned()
    }
    fn resume(&mut self) {
        self.stream.get_mut().write_all(b"c").unwrap();
    }
    fn reach(&mut self, target: &str) {
        loop {
            let event = self.get_event();
            if event == target {
                return;
            }
            assert_ne!(event, "finished", "checkpoint {target} was never reached");
            self.resume();
        }
    }
    fn finish(mut self) {
        self.reach("finished");
        self.resume();
        let status = self.wait_exit();
        writeln!(self.receipt, "exit={status}").unwrap();
        assert!(status.success(), "{status}");
    }
    fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "child did not exit within 20 seconds"
            );
            // Only an exit watchdog; schedules advance exclusively by acknowledgements.
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        let status = self.wait_exit();
        writeln!(self.receipt, "signal={:?}", status.signal()).unwrap();
        assert_eq!(status.signal(), Some(9));
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    revision: u64,
    nodes: Vec<StoredNode>,
    edges: Vec<Edge>,
    files: Vec<FileRow>,
    tokens: Tokens,
}
#[derive(Debug, PartialEq)]
struct VectorState {
    identity: Option<Sha256>,
    vector: VectorOutcome<Option<Vec<f32>>>,
    failure: VectorOutcome<Option<EmbeddingFailure>>,
    query_error: Option<QueryError>,
}
struct Fixture {
    root: TempDir,
    old: State,
    vectors: Vec<u8>,
    vector_state: VectorState,
    git_objects: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(root.path())
                .status()
                .unwrap()
                .success()
        );
        for (name, bytes) in [
            ("a.md", OLD),
            (
                "b.md",
                "<!-- letopis: {\"created_at\":1} -->\nDeletedToken\n",
            ),
            ("i.md", RETAINED),
            (
                "z.md",
                "<!-- letopis: {\"created_at\":1} -->\nStableToken\n",
            ),
        ] {
            fs::write(root.path().join(name), bytes).unwrap();
        }
        fs::write(root.path().join("source.txt"), SOURCE).unwrap();
        let git_objects = get_git_objects(root.path());
        let mut service = build_service(root.path());
        assert_eq!(service.refresh().unwrap().revision, 1);
        assert_eq!(get_git_objects(root.path()), git_objects);
        seed_vectors(&mut service.storage);
        drop(service);
        let old = get_state(root.path());
        let vectors = fs::read(root.path().join(".letopis/vectors.db")).unwrap();
        let vector_state = get_vectors(root.path());
        Self {
            root,
            old,
            vectors,
            vector_state,
            git_objects,
        }
    }
    fn edit(&self, bytes: &str) {
        fs::write(self.root.path().join("a.md"), bytes).unwrap();
        fs::write(self.root.path().join("i.md"), INVALID).unwrap();
        fs::remove_file(self.root.path().join("b.md")).unwrap();
    }
    fn invalidate_retained_file(&self) {
        fs::write(self.root.path().join("i.md"), INVALID).unwrap();
    }
    fn restore_retained_file(&self) {
        fs::write(self.root.path().join("i.md"), RETAINED).unwrap();
    }
    fn assert_preserved(&self, bytes: &str) {
        assert_eq!(
            fs::read(self.root.path().join("a.md")).unwrap(),
            bytes.as_bytes()
        );
        assert_eq!(
            fs::read(self.root.path().join("i.md")).unwrap(),
            INVALID.as_bytes()
        );
        assert_eq!(
            fs::read(self.root.path().join("source.txt")).unwrap(),
            SOURCE
        );
        assert_eq!(
            fs::read(self.root.path().join("z.md")).unwrap(),
            b"<!-- letopis: {\"created_at\":1} -->\nStableToken\n"
        );
        assert!(!self.root.path().join("b.md").exists());
        assert_eq!(get_git_objects(self.root.path()), self.git_objects);
        assert_eq!(get_vectors(self.root.path()), self.vector_state);
        assert_eq!(
            fs::read(self.root.path().join(".letopis/vectors.db")).unwrap(),
            self.vectors
        );
    }
    fn build_expected(&self, bytes: &str, token: &str) -> State {
        let mut expected = self.old.clone();
        expected.revision = 2;
        expected.nodes.retain(
            |node| !matches!(&node.metadata, NodeMetadata::Knowledge(k) if k.id.as_str() == "b.md"),
        );
        for node in &mut expected.nodes {
            match &mut node.metadata {
                NodeMetadata::Knowledge(k) if k.id.as_str() == "a.md" => {
                    k.hash = Sha256::digest(bytes.as_bytes());
                    k.search_hash = Sha256::digest(format!("{token} source").as_bytes());
                    k.created_at = 3;
                }
                NodeMetadata::Source(s) => s.created_at = 2,
                _ => {}
            }
        }
        expected.files.retain(|file| file.path.as_str() != "b.md");
        for file in &mut expected.files {
            let contents = match file.path.as_str() {
                "a.md" => bytes,
                "i.md" => INVALID,
                _ => continue,
            };
            // Independent oracle: observe the file with std, not the adapter under test.
            let metadata = fs::metadata(self.root.path().join(file.path.as_str())).unwrap();
            file.size = metadata.len();
            file.mtime_ns = metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec();
            file.inode = metadata.ino();
            file.hash = Sha256::digest(contents.as_bytes());
            file.checked_at_ns = TIME;
            if file.path.as_str() == "i.md" {
                file.valid = false;
                file.error = Some("invalid letopis metadata".to_owned());
            }
        }
        expected
            .tokens
            .retain(|(term, _, _, _)| term != "deletedtoken");
        for (term, _, _, _) in &mut expected.tokens {
            if term == "alphatoken" {
                *term = token.to_lowercase();
            }
        }
        expected.tokens.sort();
        expected
    }
}
#[test]
fn sigkill_keeps_exact_old_or_complete_new_publication() {
    for cut in CUTS {
        // Given: independently captured old state and literal expected new state.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let mut worker = Worker::spawn(fixture.root.path(), "publish");
        worker.reach(cut);
        if matches!(cut, "cleared" | "projections" | "fts" | "fingerprints") {
            // The first file is a deletion. Cut the second file after real replacements.
            worker.resume();
            worker.reach(cut);
        }
        // When: genuine process death at an acknowledged operation boundary.
        worker.kill();
        // Then: reopen, including SQLite recovery, before retrying refresh.
        let actual = get_state(fixture.root.path());
        assert_eq!(
            actual,
            if cut == "after_commit" {
                &expected
            } else {
                &fixture.old
            }
            .clone(),
            "cut={cut}"
        );
        let filesystem = LocalFileSystem::open(fixture.root.path()).unwrap();
        let guard = filesystem.lock_index().unwrap();
        drop(guard);
        fixture.assert_preserved(NEW);
        Worker::spawn(
            fixture.root.path(),
            if cut == "after_commit" {
                "noop"
            } else {
                "publish"
            },
        )
        .finish();
        assert_eq!(
            get_state(fixture.root.path()),
            expected,
            "restart cut={cut}"
        );
        fixture.assert_preserved(NEW);
    }
}

#[test]
fn duplicate_publishers_commit_once_after_real_contention() {
    // Given
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let expected = fixture.build_expected(NEW, "BravoToken");
    let mut owner = Worker::spawn(fixture.root.path(), "publish");
    owner.reach("locked");
    let mut waiter = Worker::spawn(fixture.root.path(), "noop");
    waiter.reach("contended");
    // When
    owner.resume();
    owner.finish();
    waiter.resume();
    waiter.finish();
    // Then
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(NEW);
}

#[test]
fn waiting_publisher_redetects_newer_edit_after_owner_is_killed() {
    // Given
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let mut owner = Worker::spawn(fixture.root.path(), "hold");
    owner.reach("holding");
    let mut waiter = Worker::spawn(fixture.root.path(), "publish");
    waiter.reach("contended");
    fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
    let expected = fixture.build_expected(LATEST, "CharlieToken");
    let lock_before = fs::metadata(fixture.root.path().join(".letopis/index.lock")).unwrap();
    // When
    owner.kill();
    waiter.resume();
    waiter.finish();
    // Then
    assert_eq!(
        fs::metadata(fixture.root.path().join(".letopis/index.lock"))
            .unwrap()
            .ino(),
        lock_before.ino()
    );
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(LATEST);
}

#[test]
fn noop_finishes_while_another_process_holds_index_lock() {
    // Given
    let fixture = Fixture::new();
    let mut owner = Worker::spawn(fixture.root.path(), "hold");
    owner.reach("holding");
    // When
    Worker::spawn(fixture.root.path(), "noop").finish();
    // Then
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    owner.kill();
}

#[test]
fn pinned_wal_reader_keeps_old_snapshot_and_new_reader_sees_complete_commit() {
    // Given
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let expected = fixture.build_expected(NEW, "BravoToken");
    let mut storage = build_service(fixture.root.path()).storage;
    let mut pinned = storage.begin_read().unwrap();
    let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
    fts.execute_batch("begin;").unwrap();
    assert_eq!(get_tokens(&fts), fixture.old.tokens);
    // When
    Worker::spawn(fixture.root.path(), "publish").finish();
    // Then
    assert_eq!(pinned.get_revision(), fixture.old.revision);
    assert_eq!(pinned.get_nodes().unwrap(), fixture.old.nodes);
    assert_eq!(
        pinned.get_edges_by_source_id(None).unwrap(),
        fixture.old.edges
    );
    assert_eq!(pinned.get_files().unwrap(), fixture.old.files);
    assert_eq!(get_tokens(&fts), fixture.old.tokens);
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(NEW);
}

#[test]
fn late_storage_error_rolls_back_every_cache_mutation() {
    // Given
    let fixture = Fixture::new();
    fixture.edit(NEW);
    // When
    Worker::spawn(fixture.root.path(), "fail").finish();
    // Then
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    fixture.assert_preserved(NEW);
    // When
    Worker::spawn(fixture.root.path(), "publish").finish();
    // Then
    assert_eq!(
        get_state(fixture.root.path()),
        fixture.build_expected(NEW, "BravoToken")
    );
}

#[test]
fn uncommitted_file_diagnostic_is_absent_from_status() {
    // Given: a valid publication, then one authored file becomes invalid.
    let fixture = Fixture::new();
    fixture.invalidate_retained_file();

    // When: the cache commit fails after the invalid file projection is staged.
    Worker::spawn(fixture.root.path(), "fail").finish();

    // Then: the old publication remains and a fresh status derives no file problem.
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    let status = get_status(fixture.root.path());
    assert_eq!(status.revision, Some(1));
    assert!(get_invalid_file_problems(&status).is_empty());
    assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
}

#[test]
fn failed_publication_retry_persists_the_file_diagnostic() {
    // Given: a failed publication with one invalid authored file.
    let fixture = Fixture::new();
    fixture.invalidate_retained_file();
    Worker::spawn(fixture.root.path(), "fail").finish();

    // When: the injected fault is gone and publication retries.
    Worker::spawn(fixture.root.path(), "publish").finish();

    // Then: the real safe diagnostic is persisted and survives a fresh status.
    let status = get_status(fixture.root.path());
    assert_eq!(status.revision, Some(2));
    assert_eq!(
        get_invalid_file_problems(&status),
        vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
    );
    assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
}

#[test]
fn repaired_file_problem_disappears_after_publication() {
    // Given: a publication that stored the invalid-file diagnostic.
    let fixture = Fixture::new();
    fixture.invalidate_retained_file();
    Worker::spawn(fixture.root.path(), "publish").finish();
    assert_eq!(
        get_invalid_file_problems(&get_status(fixture.root.path())),
        vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
    );

    // When: the authored file is repaired and republished.
    fixture.restore_retained_file();
    Worker::spawn(fixture.root.path(), "publish").finish();

    // Then: the current problem disappears.
    let status = get_status(fixture.root.path());
    assert_eq!(status.revision, Some(3));
    assert!(get_invalid_file_problems(&status).is_empty());
}

fn run_normal_service(root: &Path, mode: &str) {
    LocalFileSystem::configure_next_process_test(TIME, Box::new(|| reach_checkpoint("contended")));
    let mut service = wire_document_refresher(root).unwrap();
    reach_checkpoint("opened");
    let outcome = service.refresh().unwrap();
    assert_eq!(outcome.published, mode != "noop");
}

fn get_state(root: &Path) -> State {
    let mut storage = build_service(root).storage;
    let mut read = storage.begin_read().unwrap();
    let connection = Connection::open(root.join(".letopis/cache.db")).unwrap();
    State {
        revision: read.get_revision(),
        nodes: read.get_nodes().unwrap(),
        edges: read.get_edges_by_source_id(None).unwrap(),
        files: read.get_files().unwrap(),
        tokens: get_tokens(&connection),
    }
}

fn get_tokens(connection: &Connection) -> Tokens {
    connection.execute_batch("create virtual table if not exists temp.tokens using fts5vocab(main, text_index, instance);").unwrap();
    connection
        .prepare("select term, doc, col, offset from temp.tokens order by term, doc, col, offset")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn get_vectors(root: &Path) -> VectorState {
    let mut storage = build_service(root).storage;
    let mut read = storage.begin_read().unwrap();
    let identity = Sha256::digest(b"model identity");
    VectorState {
        identity: read.get_config_identity(),
        vector: read
            .get_vector_by_search_hash(&identity, &Sha256::digest(b"retained vector"))
            .unwrap(),
        failure: read
            .get_failure_by_search_hash(&identity, &Sha256::digest(b"failed vector"))
            .unwrap(),
        query_error: read.get_query_error().unwrap(),
    }
}

fn get_status(root: &Path) -> StatusReport {
    let git = GitRepository::open(root).unwrap();
    let files = LocalFileSystem::open(git.get_root()).unwrap();
    let pinned = files.open_existing_service_directory().unwrap().unwrap();
    let storage =
        SqliteStatusStorage::open_confined(&git.get_root().join(".letopis"), &pinned).unwrap();
    let provenance = ProvenanceService::new(
        GitRepository::open(root).unwrap(),
        LocalFileSystem::open(root).unwrap(),
    );
    let provider = crate::build_embedding_provider().unwrap();
    let graph = GraphDiagnosticsService::new();
    let mut status = StatusService {
        storage,
        files: &files,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    };
    status.get_status().unwrap()
}

fn get_invalid_file_problems(report: &StatusReport) -> Vec<(String, String)> {
    report
        .problems
        .iter()
        .filter_map(|problem| {
            let path = problem.details.get("path")?.as_str()?.to_owned();
            let reason = problem.details.get("reason")?.as_str()?.to_owned();
            (path == "i.md" && reason == "invalid_metadata").then_some((path, reason))
        })
        .collect()
}

fn build_service(root: &Path) -> Service {
    let git = GitRepository::open(root).unwrap();
    let mut filesystem = LocalFileSystem::open(git.get_root()).unwrap();
    filesystem.configure_process_test(TIME, Box::new(|| reach_checkpoint("contended")));
    let directory = filesystem.open_service_directory().unwrap();
    let storage =
        SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory).unwrap();
    let provenance = ProvenanceService::new(
        GitRepository::open(root).unwrap(),
        LocalFileSystem::open(root).unwrap(),
    );
    DocumentRefreshService {
        git,
        filesystem,
        markdown: MarkdownParser::new(),
        storage,
        provenance,
    }
}

fn get_git_objects(root: &Path) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["count-objects", "-v"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

fn seed_vectors(storage: &mut SqliteStorage) {
    let identity = Sha256::digest(b"model identity");
    storage.reset_vectors(None, &identity).unwrap();
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("identity changed");
    };
    unit.store_vector(&Sha256::digest(b"retained vector"), &[1.0, 0.0], 2)
        .unwrap();
    let failure = Sha256::digest(b"failed vector");
    assert!(unit.reserve_attempt(&failure, 3).unwrap());
    unit.record_failure(&failure, "safe failure").unwrap();
    unit.set_query_error(7, "safe query error").unwrap();
    unit.commit().unwrap();
}

#[path = "reindex_process.rs"]
mod reindex;
