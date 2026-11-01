use crate::{
    cli,
    support::documents::{DocumentRepo, State, build_whole},
};
use letopis::{
    domain::{RepoPath, Sha256},
    markdown::parse,
    open_filesystem, open_storage,
    service::{
        EmbedLocker, EmbeddingConfig, EmbeddingFailure, Refresher, Storage, StorageRead,
        VectorOutcome,
    },
    wire_document_refresher,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Watchdog for owned fixture completion; not a product deadline.
pub const WAIT: Duration = Duration::from_secs(10);
/// Valid native Ollama response with an exactly assertable vector.
pub const SUCCESS: &str = r#"{"model":"fixture","embeddings":[[1,2,3]]}"#;
/// Synthetic privacy sentinel; never a real credential.
pub const SECRET: &str = "private-provider-body-and-key-sentinel";

/// An isolated repository whose projection is published by the real Documents service.
pub struct Fixture {
    /// Repository and corpus used by this one test.
    pub repo: DocumentRepo,
}
impl Fixture {
    /// Create an isolated committed Git corpus with a real document projection.
    ///
    /// # Panics
    /// Panics when temporary files, Git setup or initial publication fail.
    pub fn new() -> Self {
        let fixture = Self {
            repo: DocumentRepo::new(),
        };
        fixture.repo.write(".gitignore", ".letopis/\n");
        fixture.publish("Actual product drain token");
        fixture.repo.run_git(&["add", ".gitignore", "knowledge.md"]);
        fixture.repo.run_git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.org",
            "commit",
            "-qm",
            "fixture",
        ]);
        fixture
    }

    /// Replace the fixture knowledge file and atomically refresh its projection.
    ///
    /// Returns the published search hash; the real Documents service owns locking.
    /// # Panics
    /// Panics if writing, parsing or publication fails.
    pub fn publish(&self, body: &str) -> Sha256 {
        let bytes = build_whole(1, body);
        self.repo.write("knowledge.md", &bytes);
        wire_document_refresher(self.repo.get_root())
            .unwrap()
            .refresh()
            .unwrap();
        parse(&RepoPath::parse("knowledge.md").unwrap(), bytes.as_bytes())
            .unwrap()
            .unwrap()
            .elements[0]
            .search_hash
    }

    /// Write a complete local Ollama configuration and return its validated identity.
    ///
    /// The URL must be a supported HTTP endpoint; this does not perform inference.
    /// # Panics
    /// Panics if the file cannot be written or the supplied configuration is invalid.
    pub fn write_config(&self, url: &str, retries: u64) -> EmbeddingConfig {
        let configured = json!({
            "provider":"ollama", "model":"fixture", "parameters":{"base_url":url},
            "max_retries":retries,
        });
        let bytes = serde_json::to_vec(&json!({"embedding_model": {
            "enabled":true, "provider":"ollama", "model":"fixture",
            "parameters":{"base_url":url}, "max_retries":retries,
        }}))
        .unwrap();
        self.repo.write(".letopis/config.json", &bytes);
        EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap()
    }

    /// Start the actual product drain with EOF stdin and captured output.
    ///
    /// The returned guard owns termination/reaping if a test fails.
    /// # Panics
    /// Panics if the product executable cannot be spawned.
    pub fn spawn_drain(&self) -> cli::OwnedChild {
        cli::OwnedChild::spawn(
            Command::new(env!("CARGO_BIN_EXE_letopis"))
                .args(["embed", "--drain"])
                .current_dir(self.repo.get_root())
                .env("OPENAI_API_KEY", SECRET)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
    }

    /// Start the fixture consumer using real readiness and the product executable.
    ///
    /// It waits for explicit test input after reporting the foreground result.
    /// # Panics
    /// Panics if the fixture cannot be built or its process cannot be spawned.
    pub fn spawn_foreground(&self) -> cli::OwnedChild {
        cli::OwnedChild::spawn(
            Command::new(cli::get_fixture())
                .arg("product-foreground")
                .current_dir(self.repo.get_root())
                .env("LETOPIS_FIXTURE_EXECUTABLE", env!("CARGO_BIN_EXE_letopis"))
                .env("OPENAI_API_KEY", SECRET)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
    }

    /// Start the product drain while discarding its stdout response.
    ///
    /// The owned child still captures stderr and can be awaited for completion.
    /// # Panics
    /// Panics if the product process cannot be spawned.
    pub fn spawn_lost_drain(&self) -> cli::OwnedChild {
        cli::OwnedChild::spawn(
            Command::new(env!("CARGO_BIN_EXE_letopis"))
                .args(["embed", "--drain"])
                .current_dir(self.repo.get_root())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped()),
        )
    }

    /// Release the foreground input barrier and assert successful output and EOF.
    ///
    /// Does not release any HTTP barrier, so EOF demonstrates parent independence.
    /// # Panics
    /// Panics on timeout, failed process/I/O or an unexpected response.
    pub fn finish_foreground(&self, child: &mut cli::OwnedChild) {
        child.send_input(b"exit\n");
        let output = child.collect_output(WAIT);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["event"], "product-foreground");
        assert!(response["warning"].is_null(), "{response}");
        println!("PRODUCT_FOREGROUND {response}");
    }

    /// Read the vector stored for the expected configuration and exact search hash.
    ///
    /// # Panics
    /// Panics on storage failure or an unexpected configuration identity.
    pub fn get_vector_by_search_hash(
        &self,
        config: &EmbeddingConfig,
        hash: &Sha256,
    ) -> Option<Vec<f32>> {
        unwrap_current(
            self.open_storage()
                .begin_read_tx()
                .unwrap()
                .get_vector_by_search_hash(&config.get_identity(), hash)
                .unwrap(),
        )
    }

    /// Read durable attempts and the safe diagnostic for one expected identity/hash.
    ///
    /// # Panics
    /// Panics on storage failure or an unexpected configuration identity.
    pub fn get_failure_by_search_hash(
        &self,
        config: &EmbeddingConfig,
        hash: &Sha256,
    ) -> Option<EmbeddingFailure> {
        unwrap_current(
            self.open_storage()
                .begin_read_tx()
                .unwrap()
                .get_failure_by_search_hash(&config.get_identity(), hash)
                .unwrap(),
        )
    }

    /// Assert that no eligible durable row remains under this configuration budget.
    ///
    /// # Panics
    /// Panics on storage failure, changed identity or remaining eligible work.
    pub fn assert_no_work(&self, config: &EmbeddingConfig) {
        let work = self
            .open_storage()
            .begin_read_tx()
            .unwrap()
            .get_next_embedding_work_by_id_and_max_attempts(
                &config.get_identity(),
                config.get_max_attempts(),
                None,
            )
            .unwrap();
        assert_eq!(work, VectorOutcome::Current(None));
    }

    /// Capture published state, knowledge/index bytes and user refs for equality checks.
    ///
    /// The byte-vector entries are knowledge.md followed by the Git index.
    /// # Panics
    /// Panics if storage, files or Git refs cannot be read.
    pub fn capture_corpus(&self) -> (State, Vec<Vec<u8>>, Vec<u8>) {
        let files = ["knowledge.md", ".git/index"]
            .iter()
            .map(|path| fs::read(self.repo.get_root().join(path)).unwrap())
            .collect();
        let refs = Command::new("git")
            .args(["show-ref"])
            .current_dir(self.repo.get_root())
            .output()
            .unwrap();
        assert!(refs.status.success());
        (self.repo.get_state(), files, refs.stdout)
    }

    fn open_storage(&self) -> impl Storage + use<> {
        open_storage(
            &self.repo.get_root().join(".letopis/cache.db"),
            &self.repo.get_root().join(".letopis/vectors.db"),
        )
        .unwrap()
    }
}

/// Owns cleanup of only this fixture's spawned detached product PID.
pub struct DetachedOwner {
    path: PathBuf,
    foreground: i32,
    finished: bool,
}
impl DetachedOwner {
    /// Track only the product launched by this isolated fixture foreground.
    ///
    /// No signal is sent until cleanup; the supplied PID must identify its foreground.
    /// # Panics
    /// Panics if the foreground PID cannot be represented by the native process API.
    pub fn new(root: &Path, foreground: u32) -> Self {
        Self {
            path: root.join(".letopis/embed.lock"),
            foreground: i32::try_from(foreground).unwrap(),
            finished: false,
        }
    }

    /// Wait for the known detached product to terminate, then disarm cleanup.
    ///
    /// An exited Linux zombie counts as terminated; direct calls separately test exit code.
    /// # Panics
    /// Panics if the ownership PID is invalid or the process exceeds the watchdog.
    pub fn wait_for_exit(&mut self) {
        let pid = self.get_pid();
        let deadline = Instant::now() + WAIT;
        while !has_exited(pid) {
            assert!(Instant::now() < deadline, "detached product did not exit");
            thread::sleep(Duration::from_millis(5));
        }
        self.finished = true;
        println!("PRODUCT_DRAIN {{\"pid\":{pid},\"exited\":true}}");
    }

    /// Read this isolated fixture's drain PID and reject foreground/test identities.
    ///
    /// The caller must have observed actual HTTP arrival before trusting this record.
    /// # Panics
    /// Panics for absent/malformed PID text or an excluded process identity.
    pub fn get_pid(&self) -> i32 {
        let pid = fs::read_to_string(&self.path)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        assert!(pid > 0);
        assert_ne!(pid, self.foreground);
        assert_ne!(u32::try_from(pid).unwrap(), std::process::id());
        pid
    }
}
impl Drop for DetachedOwner {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(pid) = fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
        else {
            return;
        };
        if pid <= 0
            || pid == self.foreground
            || u32::try_from(pid).ok() == Some(std::process::id())
            || has_exited(pid)
        {
            return;
        }
        // SAFETY: this PID came from this isolated fixture's ownership file and
        // differs from its foreground/test process; only its launched drain is targeted.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        let deadline = Instant::now() + WAIT;
        while !has_exited(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Assert one successful X-1 drain response and exit zero.
///
/// # Panics
/// Panics on a failed process, stderr, malformed JSON or an unexpected envelope.
pub fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response, json!({"ok":true,"result":{}}));
    println!("PRODUCT_RESPONSE {response}");
}

/// Prove lock release by actually acquiring a fresh embedding lease.
///
/// # Panics
/// Panics if the lock path is unsafe or another process retains ownership.
pub fn assert_released(root: &Path) {
    let files = open_filesystem(root).unwrap();
    assert!(files.try_lock_embed().unwrap().is_some());
}

fn has_exited(pid: i32) -> bool {
    #[cfg(target_os = "linux")]
    if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) {
        // Minimal containers may retain an orphan zombie until PID 1 reaps it.
        return stat
            .rsplit_once(") ")
            .is_some_and(|(_, rest)| rest.starts_with('Z'));
    }
    // SAFETY: signal zero only observes whether the known owned PID still exists.
    let result = unsafe { libc::kill(pid, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected configuration identity"),
    }
}
