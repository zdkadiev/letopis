use super::{Files, open_storage, publish};
use crate::{
    domain::Sha256,
    markdown::MarkdownParser,
    service::{
        DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind, EmbeddingProvider,
        EmbeddingResult, EmbeddingService, Storage, StorageError, StorageErrorKind, StorageRead,
        StorageResult, VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{
    collections::BTreeSet,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    process::{Child, Command, Output, Stdio},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const STORAGE_CASE: &str =
    "service::embedding::tests::parallel::joins_submitted_jobs_before_returning_storage_failure";
const PANIC_CASE: &str =
    "service::embedding::tests::parallel::propagates_worker_panic_after_joining_other_jobs";
const CLEANUP_PANIC_CASE: &str =
    "service::embedding::tests::parallel::propagates_worker_panic_during_error_cleanup";
const WAIT: Duration = Duration::from_secs(5);

#[test]
fn joins_submitted_jobs_before_returning_storage_failure() {
    if run_isolated(STORAGE_CASE) {
        return;
    }
    // Given: four held jobs and real storage that fails the first result publication.
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    let provider = Arc::new(ControlledProvider {
        gate: Arc::clone(&gate),
        panic_text: None,
    });
    let storage = FailingStorage {
        inner: fixture.storage,
        gate: Arc::clone(&gate),
        writes: 0,
    };
    let mut service = EmbeddingService::new(storage, fixture.files, MarkdownParser, provider);
    let (finished, received) = mpsc::channel();
    let child_gate = Arc::clone(&gate);
    let worker = thread::spawn(move || {
        let result = service.drain();
        finished.send(child_gate.get_active()).unwrap();
        (result, service.storage)
    });
    gate.wait_for(Event::Entered(4));
    // When: one result reaches a storage error while the other three remain held.
    gate.release_first();
    gate.wait_for(Event::StorageFailure);
    assert_eq!(
        received.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    gate.release_all();
    let (result, mut storage) = worker.join().unwrap();
    // Then: the original safe storage error returns only after every worker finishes.
    assert_eq!(received.recv_timeout(WAIT).unwrap(), 0);
    assert_eq!(
        result.unwrap_err().get_kind(),
        EmbeddingErrorKind::InternalError
    );
    for hash in fixture.hashes {
        let VectorOutcome::Current(failure) = storage
            .begin_read()
            .unwrap()
            .read_failure(&fixture.config.get_identity(), &hash)
            .unwrap()
        else {
            panic!("identity changed");
        };
        assert_eq!(failure.unwrap().attempts, 1);
    }
}

#[test]
fn propagates_worker_panic_after_joining_other_jobs() {
    if run_isolated(PANIC_CASE) {
        return;
    }
    // Given: four held jobs and a provider that panics on the first released input.
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    let provider = Arc::new(ControlledProvider {
        gate: Arc::clone(&gate),
        panic_text: Some("job0"),
    });
    let mut service =
        EmbeddingService::new(fixture.storage, fixture.files, MarkdownParser, provider);
    let (finished, received) = mpsc::channel();
    let child_gate = Arc::clone(&gate);
    let worker = thread::spawn(move || {
        let panicked = catch_unwind(AssertUnwindSafe(|| service.drain())).is_err();
        finished.send((panicked, child_gate.get_active())).unwrap();
    });
    gate.wait_for(Event::Entered(4));
    // When: a worker panics while three others still own outstanding requests.
    gate.release_first();
    gate.wait_for(Event::WorkerPanic);
    assert_eq!(
        received.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    gate.release_all();
    worker.join().unwrap();
    // Then: the panic is propagated after cleanup, rather than swallowed or retried.
    assert_eq!(received.recv_timeout(WAIT).unwrap(), (true, 0));
}

#[test]
fn propagates_worker_panic_during_error_cleanup() {
    if run_isolated(CLEANUP_PANIC_CASE) {
        return;
    }
    // Given: the first result hits storage failure; another worker will panic during cleanup.
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    let provider = Arc::new(ControlledProvider {
        gate: Arc::clone(&gate),
        panic_text: Some("job1"),
    });
    let storage = FailingStorage {
        inner: fixture.storage,
        gate: Arc::clone(&gate),
        writes: 0,
    };
    let mut service = EmbeddingService::new(storage, fixture.files, MarkdownParser, provider);
    let child_gate = Arc::clone(&gate);
    let worker = thread::spawn(move || {
        let controlled_panic = catch_unwind(AssertUnwindSafe(|| service.drain()))
            .err()
            .is_some_and(|payload| {
                payload.downcast_ref::<&str>() == Some(&"controlled worker programming failure")
            });
        (controlled_panic, child_gate.get_active())
    });
    gate.wait_for(Event::Entered(4));
    // When: storage fails first, then the remaining provider jobs finish or panic.
    gate.release_first();
    gate.wait_for(Event::StorageFailure);
    gate.release_all();
    // Then: cleanup observes the exact worker panic and no active requests survive it.
    assert_eq!(worker.join().unwrap(), (true, 0));
}

#[test]
fn waits_for_active_jobs_before_treating_empty_selection_as_complete() {
    // Given: reserving four zero-retry hashes makes durable work selection empty.
    let fixture = Fixture::new();
    let gate = Arc::new(Gate::default());
    gate.release_all();
    let provider = Arc::new(ControlledProvider {
        gate,
        panic_text: None,
    });
    let mut service =
        EmbeddingService::new(fixture.storage, fixture.files, MarkdownParser, provider);
    // When: the actual coordinator reaches empty selection with outstanding results.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: all four literal vectors were published, not merely joined and discarded.
    for hash in fixture.hashes {
        let vector = service
            .storage
            .begin_read()
            .unwrap()
            .read_vector(&fixture.config.get_identity(), &hash)
            .unwrap();
        assert_eq!(vector, VectorOutcome::Current(Some(vec![1., 2., 3.])));
    }
}

struct Fixture {
    _root: TempDir,
    storage: SqliteStorage,
    files: Files,
    config: EmbeddingConfig,
    hashes: Vec<Sha256>,
}
impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        let mut files = Files::new(0);
        let hashes = (0..4)
            .map(|id| {
                publish(
                    &mut storage,
                    &mut files,
                    &format!("d{id}.md"),
                    &format!("job{id}"),
                )
            })
            .collect();
        let config = files.get_configured();
        Self {
            _root: root,
            storage,
            files,
            config,
            hashes,
        }
    }
}

#[derive(Default)]
struct State {
    entered: BTreeSet<String>,
    active: usize,
    first_released: bool,
    all_released: bool,
    storage_failed: bool,
    worker_panicked: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<State>,
    wake: Condvar,
}
impl Gate {
    fn wait_for(&self, event: Event) {
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .wake
            .wait_timeout_while(state, WAIT, |state| !event.is_ready(state))
            .unwrap();
        let ready = event.is_ready(&state);
        drop(state);
        assert!(ready, "controlled worker event not reached");
    }

    fn release_first(&self) {
        self.state.lock().unwrap().first_released = true;
        self.wake.notify_all();
    }

    fn release_all(&self) {
        self.state.lock().unwrap().all_released = true;
        self.wake.notify_all();
    }

    fn get_active(&self) -> usize {
        self.state.lock().unwrap().active
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Entered(usize),
    StorageFailure,
    WorkerPanic,
}
impl Event {
    fn is_ready(&self, state: &State) -> bool {
        match self {
            Self::Entered(count) => state.entered.len() >= *count,
            Self::StorageFailure => state.storage_failed,
            Self::WorkerPanic => state.worker_panicked,
        }
    }
}

struct ControlledProvider {
    gate: Arc<Gate>,
    panic_text: Option<&'static str>,
}
impl EmbeddingProvider for ControlledProvider {
    fn validate_config(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
        Ok(())
    }

    fn embed(
        &self,
        _: &EmbeddingConfig,
        texts: &[&str],
        _: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        let first = texts[0] == "job0";
        let mut state = self.gate.state.lock().unwrap();
        state.entered.insert(texts[0].into());
        state.active += 1;
        self.gate.wake.notify_all();
        while !(state.all_released || first && state.first_released) {
            state = self.gate.wake.wait(state).unwrap();
        }
        state.active -= 1;
        if self.panic_text == Some(texts[0]) {
            state.worker_panicked = true;
            self.gate.wake.notify_all();
            drop(state);
            panic!("controlled worker programming failure");
        }
        Ok(vec![vec![1., 2., 3.]])
    }
}

struct FailingStorage {
    inner: SqliteStorage,
    gate: Arc<Gate>,
    writes: usize,
}
impl Storage for FailingStorage {
    type Read<'a> = <SqliteStorage as Storage>::Read<'a>;
    type CacheWrite<'a> = <SqliteStorage as Storage>::CacheWrite<'a>;
    type VectorWrite<'a> = <SqliteStorage as Storage>::VectorWrite<'a>;

    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.writes += 1;
        if self.writes == 5 {
            self.gate.state.lock().unwrap().storage_failed = true;
            self.gate.wake.notify_all();
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "controlled storage failure",
            ));
        }
        self.inner.begin_vector_write(expected)
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, desired)
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write()
    }
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        self.inner.begin_read()
    }
}

fn run_isolated(name: &str) -> bool {
    if std::env::var("LETOPIS_PARALLEL_CASE").as_deref() == Ok(name) {
        return false;
    }
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("LETOPIS_PARALLEL_CASE", name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = OwnedCase(Some(child)).collect();
    assert!(
        output.status.success(),
        "controlled child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("test {name} ..."))
            && stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
        "controlled child did not execute exactly the requested case: {stdout}"
    );
    true
}

struct OwnedCase(Option<Child>);
impl OwnedCase {
    fn collect(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "controlled child hung");
            thread::sleep(Duration::from_millis(5));
        }
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}
impl Drop for OwnedCase {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
