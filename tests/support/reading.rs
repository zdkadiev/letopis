use super::git::{ObjectFormat, OwnedChild, TempRepo, isolate_command};
use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    service::{EmbeddingConfig, NodeMetadata, Storage, VectorOutcome},
};
use std::{
    fs,
    io::{Seek, Write},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub struct ReadingRepo(TempRepo);
impl ReadingRepo {
    pub fn new() -> Self {
        Self(TempRepo::new(ObjectFormat::Sha1))
    }
    /// Run the actual binary with isolated Git settings and synthetic child credentials.
    ///
    /// # Panics
    /// Panics if process setup fails or the owned child exceeds the watchdog.
    pub fn invoke(&self, args: &[&str], input: &[u8]) -> Output {
        self.invoke_in(self.get_root(), args, input)
    }
    /// Run the binary from a caller-selected directory in the fixture repository.
    pub fn invoke_in(&self, directory: &Path, args: &[&str], input: &[u8]) -> Output {
        self.invoke_output(directory, args, input, Stdio::piped())
    }
    pub fn get_subdir(&self) -> &Path {
        self.0.get_subdir()
    }
    /// Run the binary with discarded stdout to test retries after a lost response.
    ///
    /// # Panics
    /// Panics if process setup fails or the owned child exceeds the watchdog.
    pub fn invoke_lost(&self, args: &[&str], input: &[u8]) -> Output {
        self.invoke_output(self.get_root(), args, input, Stdio::null())
    }

    pub fn write_knowledge(&self, path: &str, body: &str) {
        self.write(path, super::build_whole_file(body).as_bytes());
    }
    pub fn write_config(&self, provider: &str, url: &str, model: &str) -> EmbeddingConfig {
        let configured = serde_json::json!({"provider":provider,"model":model,
            "parameters":{"base_url":url},"max_retries":0});
        let raw = serde_json::json!({"embedding_model":{"enabled":true,"provider":provider,
            "model":model,"parameters":{"base_url":url},"max_retries":0}})
        .to_string();
        self.write(".letopis/config.json", raw.as_bytes());
        EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap()
    }
    pub fn seed_vectors(&self, config: &EmbeddingConfig, vectors: &[(&str, &[f32])]) {
        let mut storage = self.open_storage();
        let observed = storage.begin_read_tx().unwrap().get_config_identity();
        storage
            .reset_vectors(observed.as_ref(), &config.get_identity())
            .unwrap();
        for (id, vector) in vectors {
            let node = storage
                .begin_read_tx()
                .unwrap()
                .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
                .unwrap()
                .unwrap();
            let NodeMetadata::Knowledge(node) = node.metadata else {
                panic!("expected knowledge")
            };
            let VectorOutcome::Current(mut unit) = storage
                .begin_vector_write_tx(&config.get_identity())
                .unwrap()
            else {
                panic!("identity changed")
            };
            unit.store_vector(&node.search_hash, vector, vector.len())
                .unwrap();
            unit.commit().unwrap();
        }
    }
    pub fn open_storage(&self) -> impl Storage + use<> {
        letopis::open_storage(
            &self.get_root().join(".letopis/cache.db"),
            &self.get_root().join(".letopis/vectors.db"),
        )
        .unwrap()
    }
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.get_root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    pub fn get_root(&self) -> &Path {
        self.0.get_root()
    }
    fn invoke_output(
        &self,
        directory: &Path,
        args: &[&str],
        input: &[u8],
        stdout: Stdio,
    ) -> Output {
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(input).unwrap();
        stdin.rewind().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
        isolate_command(&mut command);
        command
            .current_dir(directory)
            .args(args)
            .stdin(Stdio::from(stdin))
            .stdout(stdout)
            .env("OPENAI_API_KEY", "fixture-secret")
            .stderr(Stdio::piped());
        OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30))
    }
}

/// Run a single test with synthetic credentials isolated from the multithreaded parent.
pub fn run_with_credentials(test: &str) -> bool {
    if std::env::var_os("LETOPIS_SEARCH_CHILD").is_some() {
        return false;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    isolate_command(&mut command);
    command
        .args(["--exact", test, "--nocapture"])
        .env("LETOPIS_SEARCH_CHILD", "1")
        .env("OPENAI_API_KEY", "fixture-secret")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(45));
    assert!(
        output.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}
