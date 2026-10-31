use super::{Files, open_storage, publish};
use crate::{
    domain::{RepoPath, Sha256},
    markdown::parse,
    service::{
        ConfigState, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingExecutor, QueryEmbedding,
        Storage, StorageRead, VectorOutcome, VectorWrite,
    },
    wire_embedder,
};
use rusqlite::{Connection, types::Value};
use std::{
    env, fs,
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{
        fs::MetadataExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const LIMIT: Duration = Duration::from_secs(15);
const CHILD: &str = "service::embedding::tests::process::run_child";
const SUCCESS: &str = r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#;

#[test]
#[ignore = "private child entrypoint explicitly invoked by process parents"]
fn run_child() {
    // Given
    reach_checkpoint("child");
    let root = PathBuf::from(env::var_os("LETOPIS_PROCESS_ROOT").unwrap());
    let storage = crate::open_storage(&root.join("cache"), &root.join("vectors")).unwrap();
    let mut service = wire_embedder(&root, storage).unwrap();
    // When / Then: each action is checked in its assertion.
    if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("query") {
        assert_eq!(
            service.embed_query("canary query").unwrap(),
            QueryEmbedding::Unavailable
        );
    } else if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("readiness") {
        assert!(!service.has_work().unwrap());
    } else if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("configuration") {
        assert!(matches!(
            service.get_configuration().unwrap(),
            ConfigState::Changed
        ));
    } else if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("observed") {
        assert!(matches!(
            service.get_configuration().unwrap(),
            ConfigState::Changed | ConfigState::Ready(_)
        ));
    } else {
        let expected = match env::var("LETOPIS_PROCESS_EXPECT").unwrap().as_str() {
            "Complete" => DrainOutcome::Complete,
            "AlreadyRunning" => DrainOutcome::AlreadyRunning,
            "ConfigChanged" => DrainOutcome::ConfigChanged,
            other => panic!("unknown expected outcome {other}"),
        };
        assert_eq!(Embedder::drain(&mut service).unwrap(), expected);
    }
}

struct Fixture {
    root: TempDir,
    socket_dir: TempDir,
    listener: TcpListener,
    hash: Sha256,
    config: EmbeddingConfig,
}
impl Fixture {
    fn new(retries: u64) -> Self {
        let root = TempDir::new().unwrap();
        let socket_dir = TempDir::new_in("/tmp").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut storage = open_storage(&root);
        let mut files = Files::new(retries);
        let hash = publish(&mut storage, &mut files, "a.md", "knowledge canary");
        fs::write(root.path().join("a.md"), &files.documents["a.md"]).unwrap();
        fs::create_dir(root.path().join(".letopis")).unwrap();
        let config = Self::write_config(&root, &listener, retries);
        Self {
            root,
            socket_dir,
            listener,
            hash,
            config,
        }
    }
    fn write_config(root: &TempDir, listener: &TcpListener, retries: u64) -> EmbeddingConfig {
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        let configured = serde_json::json!({
            "provider":"ollama", "model":"fixture", "max_retries": retries,
            "parameters":{"base_url":url}
        });
        let bytes = serde_json::to_vec(&serde_json::json!({"embedding_model": {
            "enabled":true, "provider":"ollama", "model":"fixture",
            "parameters":{"base_url":url}, "max_retries": retries,
        }}))
        .unwrap();
        fs::write(root.path().join(".letopis/config.json"), &bytes).unwrap();
        EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap()
    }
    fn accept_http(&self) -> TcpStream {
        let start = Instant::now();
        let stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(start.elapsed() < LIMIT, "HTTP readiness timeout");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("HTTP accept: {error}"),
            }
        };
        get_http_request(stream)
    }
    fn assert_state(
        &self,
        config: &EmbeddingConfig,
        vector: Option<Vec<f32>>,
        attempts: Option<u64>,
    ) {
        let mut storage = open_storage(&self.root);
        let mut read = storage.begin_read().unwrap();
        assert_eq!(
            read.get_vector_by_search_hash(&config.get_identity(), &self.hash)
                .unwrap(),
            VectorOutcome::Current(vector)
        );
        let VectorOutcome::Current(failure) = read
            .get_failure_by_search_hash(&config.get_identity(), &self.hash)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(failure.map(|failure| failure.attempts), attempts);
    }
    fn capture_cache(&self) -> Vec<Vec<Vec<Value>>> {
        let connection = Connection::open(self.root.path().join("cache")).unwrap();
        [
            "nodes",
            "edges",
            "files",
            "project",
            "text_index_data",
            "text_index_idx",
            "text_index_docsize",
            "text_index_config",
        ]
        .iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("select * from {table} order by 1"))
                .unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .collect::<Result<Vec<Vec<Value>>, _>>()
                .unwrap()
        })
        .collect()
    }
}

struct Process {
    child: Child,
    listener: UnixListener,
    log: PathBuf,
}
impl Process {
    fn spawn(fixture: &Fixture, label: &str, stage: &str, action: &str, expected: &str) -> Self {
        let socket = fixture.socket_dir.path().join(label);
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let directory = env::var_os("LETOPIS_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| fixture.root.path().join("receipts"));
        fs::create_dir_all(&directory).unwrap();
        let log = directory.join(format!(
            "{}-{label}.log",
            fixture.root.path().file_name().unwrap().to_string_lossy()
        ));
        let output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log)
            .unwrap();
        let child = Command::new(env::current_exe().unwrap())
            .args([CHILD, "--ignored", "--exact", "--nocapture"])
            .env("LETOPIS_PROCESS_ROOT", fixture.root.path())
            .env("LETOPIS_PROCESS_SOCKET", socket)
            .env("LETOPIS_PROCESS_STAGE", stage)
            .env("LETOPIS_PROCESS_ACTION", action)
            .env("LETOPIS_PROCESS_EXPECT", expected)
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        Self {
            child,
            listener,
            log,
        }
    }
    fn await_checkpoint(&mut self) -> UnixStream {
        let start = Instant::now();
        let mut stream = loop {
            if let Ok((stream, _)) = self.listener.accept() {
                break stream;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "child exited: {}",
                fs::read_to_string(&self.log).unwrap()
            );
            assert!(start.elapsed() < LIMIT, "checkpoint timeout");
            thread::sleep(Duration::from_millis(5));
        };
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(LIMIT)).unwrap();
        stream.set_write_timeout(Some(LIMIT)).unwrap();
        let mut signal = [0];
        stream.read_exact(&mut signal).unwrap();
        assert_eq!(signal, *b"R");
        stream
    }
    fn finish(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "child failed: {}",
                    fs::read_to_string(&self.log).unwrap()
                );
                return;
            }
            assert!(start.elapsed() < LIMIT, "child watchdog timeout");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn kill(&mut self) {
        self.child.kill().unwrap();
        assert!(!self.child.wait().unwrap().success());
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // These filtered children spawn no descendants. Reap even on parent panic.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
#[test]
fn two_drains_have_one_owner_and_one_stored_vector() {
    // Given
    let fixture = Fixture::new(0);
    let before = fixture.capture_cache();
    let mut owner = Process::spawn(&fixture, "owner", "reserved", "drain", "Complete");
    let mut barrier = owner.await_checkpoint();
    // When
    let mut contender = Process::spawn(&fixture, "contender", "", "drain", "AlreadyRunning");
    contender.finish();
    fixture.assert_state(&fixture.config, None, Some(1));
    assert!(fixture.listener.accept().is_err());
    barrier.write_all(b"C").unwrap();
    reply(fixture.accept_http(), SUCCESS);
    owner.finish();
    // Then
    fixture.assert_state(&fixture.config, Some(vec![1., 2., 3.]), None);
    assert_eq!(fixture.capture_cache(), before);
    assert!(fixture.listener.accept().is_err());
}

#[test]
fn killed_reservation_keeps_budget_and_releases_stable_lock() {
    // Given
    let fixture = Fixture::new(0);
    let before = fixture.capture_cache();
    let mut owner = Process::spawn(&fixture, "killed", "reserved", "drain", "Complete");
    let _barrier = owner.await_checkpoint();
    let lock = fixture.root.path().join(".letopis/embed.lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    fixture.assert_state(&fixture.config, None, Some(1));
    // When
    owner.kill();
    let mut restarted = Process::spawn(&fixture, "restart", "", "drain", "Complete");
    restarted.finish();
    // Then
    fixture.assert_state(&fixture.config, None, Some(1));
    assert_eq!(fs::metadata(lock).unwrap().ino(), inode);
    assert!(fixture.listener.accept().is_err());
    assert_eq!(fixture.capture_cache(), before);
}

#[test]
fn edit_before_send_skips_stale_hash_without_reservation() {
    // Given
    let fixture = Fixture::new(0);
    let before = fixture.capture_cache();
    let mut child = Process::spawn(&fixture, "edit", "prepare", "drain", "Complete");
    let mut barrier = child.await_checkpoint();
    // When
    fs::write(
        fixture.root.path().join("a.md"),
        "<!-- letopis: {\"created_at\":1} -->\nedited canary",
    )
    .unwrap();
    barrier.write_all(b"C").unwrap();
    // In a guard mutation an HTTP request must complete so the state oracle fails.
    let mut requests = 0;
    let start = Instant::now();
    while child.child.try_wait().unwrap().is_none() && start.elapsed() < LIMIT {
        if let Ok((stream, _)) = fixture.listener.accept() {
            requests += 1;
            reply(get_http_request(stream), SUCCESS);
        }
        thread::sleep(Duration::from_millis(5));
    }
    child.finish();
    while let Ok((stream, _)) = fixture.listener.accept() {
        requests += 1;
        reply(get_http_request(stream), SUCCESS);
    }
    // Then: check stored state first to retain the compiling mutation oracle.
    fixture.assert_state(&fixture.config, None, None);
    assert_eq!(fixture.capture_cache(), before);
    assert_eq!(requests, 0, "stale text must never reach HTTP");
}

#[test]
fn reset_during_http_discards_late_document_success_and_error() {
    for (label, body) in [
        ("late-vector", SUCCESS),
        ("late-error", "private-provider-canary"),
    ] {
        // Given
        let fixture = Fixture::new(0);
        let mut child = Process::spawn(&fixture, label, "", "drain", "ConfigChanged");
        let stream = fixture.accept_http();
        fixture.assert_state(&fixture.config, None, Some(1));
        // When
        let next = reset_with_current_state(&fixture);
        let before = fixture.capture_cache();
        reply(stream, body);
        // Then: check stored state before child outcome, for mutation sensitivity.
        wait_for_exit(&mut child);
        fixture.assert_state(&next, None, None);
        assert_current_vector(&fixture, &next);
        assert_current_error(&fixture);
        assert_eq!(fixture.capture_cache(), before);
        child.finish();
    }
}

#[test]
fn reset_during_http_preserves_current_query_error() {
    for (label, body) in [
        ("query-success", SUCCESS),
        ("query-error", "private-provider-canary"),
    ] {
        // Given
        let fixture = Fixture::new(0);
        let mut child = Process::spawn(&fixture, label, "", "query", "");
        let stream = fixture.accept_http();
        // When
        let next = reset_with_current_state(&fixture);
        let before = fixture.capture_cache();
        reply(stream, body);
        wait_for_exit(&mut child);
        // Then
        assert_current_error(&fixture);
        fixture.assert_state(&next, None, None);
        assert_current_vector(&fixture, &next);
        assert_eq!(fixture.capture_cache(), before);
        child.finish();
    }
}

fn assert_current_vector(fixture: &Fixture, config: &EmbeddingConfig) {
    let mut storage = open_storage(&fixture.root);
    let mut read = storage.begin_read().unwrap();
    let bytes = fs::read(fixture.root.path().join("b.md")).unwrap();
    let parsed = parse(&RepoPath::parse("b.md").unwrap(), &bytes)
        .unwrap()
        .unwrap();
    let hash = parsed.elements[0].search_hash;
    assert_eq!(
        read.get_vector_by_search_hash(&config.get_identity(), &hash)
            .unwrap(),
        VectorOutcome::Current(Some(vec![4., 5., 6.]))
    );
}

pub(super) fn reach_checkpoint(stage: &str) {
    if env::var("LETOPIS_PROCESS_STAGE").as_deref() != Ok(stage) {
        return;
    }
    let mut socket = UnixStream::connect(env::var_os("LETOPIS_PROCESS_SOCKET").unwrap()).unwrap();
    socket.set_read_timeout(Some(LIMIT)).unwrap();
    socket.set_write_timeout(Some(LIMIT)).unwrap();
    socket.write_all(b"R").unwrap();
    let mut signal = [0];
    socket.read_exact(&mut signal).unwrap();
    assert_eq!(signal, *b"C");
}

fn get_http_request(mut stream: TcpStream) -> TcpStream {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(LIMIT)).unwrap();
    stream.set_write_timeout(Some(LIMIT)).unwrap();
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 16384);
    }
    let headers = String::from_utf8(bytes).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    assert!(length < 16384);
    stream.read_exact(&mut vec![0; length]).unwrap();
    stream
}

fn reply(mut stream: TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

fn reset_with_current_state(fixture: &Fixture) -> EmbeddingConfig {
    let next = Fixture::write_config(&fixture.root, &fixture.listener, 1);
    let mut service = wire_embedder(fixture.root.path(), open_storage(&fixture.root)).unwrap();
    service.get_configuration().unwrap();
    let mut storage = open_storage(&fixture.root);
    let mut files = Files::new(0);
    let current = publish(&mut storage, &mut files, "b.md", "current knowledge");
    fs::write(fixture.root.path().join("b.md"), &files.documents["b.md"]).unwrap();
    let VectorOutcome::Current(mut unit) =
        storage.begin_vector_write(&next.get_identity()).unwrap()
    else {
        panic!()
    };
    unit.store_vector(&current, &[4., 5., 6.], 3).unwrap();
    unit.set_query_error(7, "current safe error").unwrap();
    unit.commit().unwrap();
    next
}

fn wait_for_exit(child: &mut Process) {
    let start = Instant::now();
    while child.child.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < LIMIT, "child watchdog timeout");
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_current_error(fixture: &Fixture) {
    let mut storage = open_storage(&fixture.root);
    let error = storage
        .begin_read()
        .unwrap()
        .get_query_error()
        .unwrap()
        .unwrap();
    assert_eq!(error.at, 7);
    assert_eq!(error.error, "current safe error");
}

mod configuration;
