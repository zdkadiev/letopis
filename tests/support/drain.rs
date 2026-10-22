use crate::{
    cli,
    support::documents::{DocumentRepo, State, build_whole},
};
use letopis::{
    domain::{RepoPath, Sha256},
    markdown::parse,
    open_documents, open_filesystem, open_storage,
    service::{
        Documents, EmbedLocks, EmbeddingConfig, EmbeddingFailure, Storage, StorageRead,
        VectorOutcome,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const WAIT: Duration = Duration::from_secs(10);
pub const SUCCESS: &str = r#"{"model":"fixture","embeddings":[[1,2,3]]}"#;
pub const SECRET: &str = "private-provider-body-and-key-sentinel";

pub struct Fixture {
    pub repo: DocumentRepo,
}
impl Fixture {
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

    pub fn publish(&self, body: &str) -> Sha256 {
        let bytes = build_whole(1, body);
        self.repo.write("knowledge.md", &bytes);
        open_documents(self.repo.get_root())
            .unwrap()
            .refresh()
            .unwrap();
        parse(&RepoPath::parse("knowledge.md").unwrap(), bytes.as_bytes())
            .unwrap()
            .unwrap()
            .elements[0]
            .search_hash
    }

    pub fn write_config(&self, url: &str, retries: u64) -> EmbeddingConfig {
        let bytes = serde_json::to_vec(&json!({
            "provider":"ollama", "model":"fixture", "parameters":{"base_url":url},
            "max_retries":retries,
        }))
        .unwrap();
        self.repo.write(".letopis/config.json", &bytes);
        EmbeddingConfig::parse(&bytes).unwrap()
    }

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

    pub fn open_storage(&self) -> impl Storage + use<> {
        open_storage(
            &self.repo.get_root().join(".letopis/cache.db"),
            &self.repo.get_root().join(".letopis/vectors.db"),
        )
        .unwrap()
    }

    pub fn read_vector(&self, config: &EmbeddingConfig, hash: &Sha256) -> Option<Vec<f32>> {
        unwrap_current(
            self.open_storage()
                .begin_read()
                .unwrap()
                .read_vector(&config.get_identity(), hash)
                .unwrap(),
        )
    }

    pub fn read_failure(
        &self,
        config: &EmbeddingConfig,
        hash: &Sha256,
    ) -> Option<EmbeddingFailure> {
        unwrap_current(
            self.open_storage()
                .begin_read()
                .unwrap()
                .read_failure(&config.get_identity(), hash)
                .unwrap(),
        )
    }

    pub fn assert_no_work(&self, config: &EmbeddingConfig) {
        let work = self
            .open_storage()
            .begin_read()
            .unwrap()
            .read_next_embedding_work(&config.get_identity(), config.get_max_attempts(), None)
            .unwrap();
        assert_eq!(work, VectorOutcome::Current(None));
    }

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
        (self.repo.read_state(), files, refs.stdout)
    }
}

/// Owns cleanup of only this fixture's spawned detached product PID.
pub struct DetachedOwner {
    path: PathBuf,
    foreground: i32,
    finished: bool,
}
impl DetachedOwner {
    pub fn new(root: &Path, foreground: u32) -> Self {
        Self {
            path: root.join(".letopis/embed.lock"),
            foreground: i32::try_from(foreground).unwrap(),
            finished: false,
        }
    }

    pub fn read_pid(&self) -> i32 {
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

    pub fn wait_for_exit(&mut self) {
        let pid = self.read_pid();
        let deadline = Instant::now() + WAIT;
        while !has_exited(pid) {
            assert!(Instant::now() < deadline, "detached product did not exit");
            thread::sleep(Duration::from_millis(5));
        }
        self.finished = true;
        println!("PRODUCT_DRAIN {{\"pid\":{pid},\"exited\":true}}");
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

pub fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response, json!({"ok":true,"result":{}}));
    println!("PRODUCT_RESPONSE {response}");
}

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
