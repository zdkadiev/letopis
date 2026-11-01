use super::SqliteStorage;
use crate::{
    cli::ResponseWriter,
    domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256},
    process::SigintListener,
    service::{
        CacheWrite, CommandError, CommandErrorKind, FileRow, KnowledgeMetadata, NodeMetadata,
        Storage, StorageRead, StoredNode,
    },
};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    ffi::{c_int, c_void},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::AsRawFd,
        unix::net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(20);
const BEFORE_CHILD: &str = "storage::sqlite::cancellation_tests::before_commit_child";
const AFTER_CHILD: &str = "storage::sqlite::cancellation_tests::after_commit_child";
const SQL_CHILD: &str = "storage::sqlite::cancellation_tests::sqlite_step_child";
const CHILD_CACHE: &str = "LETOPIS_CANCELLATION_CACHE";
const CHILD_VECTORS: &str = "LETOPIS_CANCELLATION_VECTORS";
const CHILD_SOCKET: &str = "LETOPIS_CANCELLATION_SOCKET";
const CHILD_OUTPUT: &str = "LETOPIS_CANCELLATION_OUTPUT";
const CANCELLED_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";

#[test]
fn cancels_inside_sqlite_step() {
    // Given: a real staged cache unit and a progress callback that blocks inside SQL.
    let fixture = Fixture::new();
    publish(&fixture.paths, "a.md", "OldToken");
    let mut child = fixture.spawn(SQL_CHILD);
    let mut control = fixture.accept();
    assert_eq!(get_event(&mut control), "staged");
    assert_eq!(get_event(&mut control), "inside-sql");
    // When
    child.send_sigint();
    let status = child.wait_exit();
    // Then: the process died from handled cancellation before the transaction committed.
    assert_handled_exit(&status);
    assert_cancelled_file(&fixture.paths.output);
    assert_old_publication(&fixture.paths);
    publish(&fixture.paths, "later.md", "LaterToken");
    assert_later_publication(&fixture.paths, 2);
}

#[test]
fn termination_before_commit_keeps_old_publication() {
    // Given: a real cache unit staged but held before commit until process death.
    let fixture = Fixture::new();
    publish(&fixture.paths, "a.md", "OldToken");
    let mut child = fixture.spawn(BEFORE_CHILD);
    let mut control = fixture.accept();
    assert_eq!(get_event(&mut control), "staged");
    // When
    child.send_sigint();
    let status = child.wait_exit();
    // Then: the old publication survives and a later publication still succeeds.
    assert_handled_exit(&status);
    assert_cancelled_file(&fixture.paths.output);
    assert_old_publication(&fixture.paths);
    publish(&fixture.paths, "later.md", "LaterToken");
    assert_later_publication(&fixture.paths, 2);
}

#[test]
fn termination_after_commit_keeps_new_publication() {
    // Given: the child acknowledges only after its cache commit returned.
    let fixture = Fixture::new();
    publish(&fixture.paths, "a.md", "OldToken");
    let mut child = fixture.spawn(AFTER_CHILD);
    let mut control = fixture.accept();
    let acknowledgement = get_event(&mut control);
    assert_eq!(acknowledgement, "committed 2");
    // When
    child.send_sigint();
    let status = child.wait_exit();
    // Then: cancellation never rolls back the committed publication.
    assert_handled_exit(&status);
    assert_cancelled_file(&fixture.paths.output);
    assert_new_publication(&fixture.paths);
    publish(&fixture.paths, "later.md", "LaterToken");
    assert_later_publication(&fixture.paths, 3);
}

#[test]
#[ignore = "private SQL-step child, invoked only by cancels_inside_sqlite_step"]
fn sqlite_step_child() {
    let Some(paths) = Paths::from_environment() else {
        return;
    };
    // Given: a parent-owned publication and cancellation output.
    let mut storage = open_storage(&paths);
    let _listener = install_listener(&paths.output);
    let control = UnixStream::connect(&paths.socket).expect("control connection");
    write_event(control.try_clone().expect("barrier clone"), "staged");
    let barrier = register_sql_barrier(&storage.cache_writer, control);
    // When: stage real changes at the selected cancellation boundary.
    let mut unit = storage.begin_cache_write_tx().expect("cache unit");
    stage(&mut unit, "b.md", "NewToken");
    // Then: the armed callback holds the SQL step until process termination.
    barrier.arm();
    let _ = unit.commit();
}

#[test]
#[ignore = "private before-commit child, invoked only by termination_before_commit_keeps_old_publication"]
fn before_commit_child() {
    let Some(paths) = Paths::from_environment() else {
        return;
    };
    // Given: a parent-owned publication and cancellation output.
    let mut storage = open_storage(&paths);
    let _listener = install_listener(&paths.output);
    let mut control = UnixStream::connect(&paths.socket).expect("control connection");
    // When: stage real changes at the selected cancellation boundary.
    let mut unit = storage.begin_cache_write_tx().expect("cache unit");
    stage(&mut unit, "b.md", "NewToken");
    // Then: keep the transaction uncommitted until process termination.
    writeln!(control, "staged").expect("staged acknowledgement");
    hold_until_parent_closes(control);
}

#[test]
#[ignore = "private after-commit child, invoked only by termination_after_commit_keeps_new_publication"]
fn after_commit_child() {
    let Some(paths) = Paths::from_environment() else {
        return;
    };
    // Given: a parent-owned publication and cancellation output.
    let mut storage = open_storage(&paths);
    let _listener = install_listener(&paths.output);
    let mut control = UnixStream::connect(&paths.socket).expect("control connection");
    // When: stage real changes at the selected cancellation boundary.
    let mut unit = storage.begin_cache_write_tx().expect("cache unit");
    stage(&mut unit, "b.md", "NewToken");
    let revision = unit.commit().expect("committed publication");
    // Then: acknowledge only the completed commit before awaiting cancellation.
    writeln!(control, "committed {revision}").expect("commit acknowledgement");
    hold_until_parent_closes(control);
}

fn publish(paths: &Paths, id: &str, text: &str) -> u64 {
    let mut storage = open_storage(paths);
    let mut unit = storage.begin_cache_write_tx().expect("cache unit");
    stage(&mut unit, id, text);
    unit.commit().expect("publication commit")
}

fn stage<W: CacheWrite>(unit: &mut W, id: &str, text: &str) {
    let knowledge = KnowledgeId::parse(id).expect("knowledge id");
    let hash = Sha256::digest(text.as_bytes());
    unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
        uri: knowledge.clone(),
        id: knowledge.clone(),
        kind: ElementKind::Vertex,
        hash,
        created_at: 1,
        source_snapshot: None,
        lines: LineRange::new(1, 1).expect("line range"),
        search_hash: hash,
    }))
    .expect("node");
    unit.replace_text(&knowledge, text).expect("text");
    unit.upsert_file(&FileRow {
        path: RepoPath::parse(id).expect("file path"),
        size: u64::try_from(text.len()).expect("text size"),
        mtime_ns: 11,
        inode: 13,
        hash,
        checked_at_ns: 19,
        valid: true,
        error: None,
    })
    .expect("file row");
}

// The safe `rusqlite` progress handler is compiled only with its `hooks`
// feature, which this product dependency does not enable. This private child
// therefore registers the pinned FFI directly and leaks its context until the
// process exits, so every callback sees live state, including the blocking one.
fn register_sql_barrier(connection: &Connection, control: UnixStream) -> ProgressBarrier {
    let state = Box::into_raw(Box::new(ProgressState {
        armed: AtomicBool::new(false),
        control: Mutex::new(Some(control)),
    }));
    // SAFETY: `connection` is live and owns the handler registration. The boxed
    // context is intentionally leaked for this private child process, so its
    // address stays valid for every callback invocation, including the blocking
    // one that ends only when this process exits.
    unsafe {
        rusqlite::ffi::sqlite3_progress_handler(
            connection.handle(),
            1,
            Some(sqlite_progress),
            state.cast(),
        );
    }
    ProgressBarrier { state }
}

struct ProgressState {
    armed: AtomicBool,
    control: Mutex<Option<UnixStream>>,
}

struct ProgressBarrier {
    state: *mut ProgressState,
}
impl ProgressBarrier {
    fn arm(&self) {
        // SAFETY: the pointer is the leaked context registered above for this process.
        unsafe { (*self.state).armed.store(true, Ordering::SeqCst) };
    }
}

unsafe extern "C" fn sqlite_progress(context: *mut c_void) -> c_int {
    // SAFETY: SQLite passes the exact leaked context registered above; it lives
    // until this private child process exits even when the callback blocks.
    let state = unsafe { &*context.cast::<ProgressState>() };
    if !state.armed.load(Ordering::SeqCst) {
        return 0;
    }
    let Ok(mut control) = state.control.lock() else {
        return 0;
    };
    let Some(mut stream) = control.take() else {
        return 0;
    };
    let _ = writeln!(stream, "inside-sql");
    let mut byte = [0];
    let _ = stream.read(&mut byte);
    0
}

fn hold_until_parent_closes(mut control: UnixStream) {
    let mut byte = [0];
    let _ = control.read(&mut byte);
}

fn install_listener(output: &Path) -> SigintListener {
    let mut output = File::create(output).expect("cancellation output");
    let writer = ResponseWriter::default();
    SigintListener::install(Box::new(move || {
        let response = Err(CommandError::new(
            CommandErrorKind::Cancelled,
            "command cancelled",
        ));
        let _ = writer.write_response(&mut output, response);
    }))
    .expect("listener")
}

struct Publication {
    revision: u64,
    nodes: Vec<String>,
    files: Vec<String>,
    old: Vec<String>,
    new: Vec<String>,
    later: Vec<String>,
}

fn assert_old_publication(paths: &Paths) {
    let published = get_publication(paths);
    assert_eq!(published.revision, 1);
    assert_eq!(published.nodes, ["a.md"]);
    assert_eq!(published.files, ["a.md"]);
    assert_eq!(published.old, ["a.md"]);
    assert!(published.new.is_empty());
    assert!(published.later.is_empty());
}

fn assert_new_publication(paths: &Paths) {
    let published = get_publication(paths);
    assert_eq!(published.revision, 2);
    assert_eq!(published.nodes, ["a.md", "b.md"]);
    assert_eq!(published.files, ["a.md", "b.md"]);
    assert_eq!(published.old, ["a.md"]);
    assert_eq!(published.new, ["b.md"]);
    assert!(published.later.is_empty());
}

fn assert_later_publication(paths: &Paths, revision: u64) {
    let published = get_publication(paths);
    assert_eq!(published.revision, revision);
    assert!(published.nodes.iter().any(|id| id == "later.md"));
    assert_eq!(published.later, ["later.md"]);
}

fn get_publication(paths: &Paths) -> Publication {
    let mut storage = open_storage(paths);
    assert_integrity(&storage);
    let mut read = storage.begin_read_tx().expect("read transaction");
    let revision = read.get_revision();
    let nodes = read
        .get_nodes()
        .expect("nodes")
        .iter()
        .map(get_node_id)
        .collect();
    let files = read
        .get_files()
        .expect("files")
        .iter()
        .map(|file| file.path.as_str().to_owned())
        .collect();
    Publication {
        revision,
        nodes,
        files,
        old: get_text_matches_by_expression(&mut read, "OldToken"),
        new: get_text_matches_by_expression(&mut read, "NewToken"),
        later: get_text_matches_by_expression(&mut read, "LaterToken"),
    }
}

fn get_node_id(node: &StoredNode) -> String {
    match &node.metadata {
        NodeMetadata::Knowledge(knowledge) => knowledge.id.as_str().to_owned(),
        NodeMetadata::Source(source) => source.uri.as_str().to_owned(),
    }
}

fn get_text_matches_by_expression(read: &mut impl StorageRead, expression: &str) -> Vec<String> {
    read.get_text_matches_by_expression(expression)
        .expect("FTS expression")
        .into_iter()
        .map(|found| found.id.as_str().to_owned())
        .collect()
}

fn assert_integrity(storage: &SqliteStorage) {
    for connection in [&storage.cache_writer, &storage.vector_writer] {
        let result: String = connection
            .query_row("pragma quick_check", [], |row| row.get(0))
            .expect("quick_check");
        assert_eq!(result, "ok");
    }
}

fn open_storage(paths: &Paths) -> SqliteStorage {
    SqliteStorage::open(&paths.cache, &paths.vectors).expect("storage")
}

fn assert_handled_exit(status: &ExitStatus) {
    assert_eq!(status.code(), Some(1), "unhandled signal exit: {status:?}");
}

fn assert_cancelled_file(output: &Path) {
    let bytes = fs::read(output).expect("cancellation output");
    assert_eq!(bytes, CANCELLED_ENVELOPE);
    let response: Value = serde_json::from_slice(&bytes).expect("one JSON value");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "cancelled");
}

struct Paths {
    cache: PathBuf,
    vectors: PathBuf,
    socket: PathBuf,
    output: PathBuf,
}
impl Paths {
    fn from_environment() -> Option<Self> {
        Some(Self {
            cache: PathBuf::from(std::env::var_os(CHILD_CACHE)?),
            vectors: PathBuf::from(std::env::var_os(CHILD_VECTORS)?),
            socket: PathBuf::from(std::env::var_os(CHILD_SOCKET)?),
            output: PathBuf::from(std::env::var_os(CHILD_OUTPUT)?),
        })
    }
}

struct Fixture {
    _root: TempDir,
    _socket: TempDir,
    listener: UnixListener,
    paths: Paths,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("storage root");
        let socket = tempfile::Builder::new()
            .prefix("tk29-storage-")
            .tempdir_in("/tmp")
            .expect("socket directory");
        let address = socket.path().join("control.sock");
        let listener = UnixListener::bind(&address).expect("control listener");
        Self {
            paths: Paths {
                cache: root.path().join("cache.db"),
                vectors: root.path().join("vectors.db"),
                socket: address,
                output: root.path().join("cancelled.json"),
            },
            _root: root,
            _socket: socket,
            listener,
        }
    }

    fn spawn(&self, child: &str) -> OwnedChild {
        let process = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", child, "--ignored", "--nocapture"])
            .env(CHILD_CACHE, &self.paths.cache)
            .env(CHILD_VECTORS, &self.paths.vectors)
            .env(CHILD_SOCKET, &self.paths.socket)
            .env(CHILD_OUTPUT, &self.paths.output)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("child spawn");
        OwnedChild(Some(process))
    }

    fn accept(&self) -> BufReader<UnixStream> {
        wait_readable(self.listener.as_raw_fd());
        let (stream, _) = self.listener.accept().expect("control connection");
        stream
            .set_read_timeout(Some(WAIT))
            .expect("control read timeout");
        stream
            .set_write_timeout(Some(WAIT))
            .expect("control write timeout");
        BufReader::new(stream)
    }
}

fn get_event(control: &mut BufReader<UnixStream>) -> String {
    let mut line = String::new();
    assert!(
        control.read_line(&mut line).expect("control event") > 0,
        "child closed before acknowledgement"
    );
    line.trim().to_owned()
}

fn write_event(mut control: UnixStream, event: &str) {
    writeln!(control, "{event}").expect("control event");
}

fn wait_readable(fd: i32) {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `poll` points to one initialized entry for the live listener.
    let ready = unsafe {
        libc::poll(
            &raw mut poll,
            1,
            i32::try_from(WAIT.as_millis()).expect("poll timeout"),
        )
    };
    assert_eq!(ready, 1, "child did not connect");
}

struct OwnedChild(Option<Child>);
impl OwnedChild {
    fn send_sigint(&mut self) {
        let child = self.0.as_mut().expect("live child");
        assert!(
            child.try_wait().expect("child status").is_none(),
            "child already exited"
        );
        let pid = i32::try_from(child.id()).expect("child pid");
        // SAFETY: the unreaped child is owned here, so its PID cannot be reused
        // before collection; signal only that process, not its group.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
    }

    fn wait_exit(&mut self) -> ExitStatus {
        let child = self.0.as_mut().expect("live child");
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = child.try_wait().expect("child status") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not exit within the watchdog");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
