use super::{EmbeddingService, RETRY_DELAY};
use crate::{
    domain::{ElementKind, KnowledgeId, RepoPath, Sha256},
    markdown::{MarkdownParser, parse},
    service::{
        CacheWrite, ConfigState, Configuration, DrainOutcome, Embedder, EmbeddingConfig,
        EmbeddingError, EmbeddingErrorKind, EmbeddingFiles, EmbeddingProvider, EmbeddingResult,
        KnowledgeMetadata, NodeMetadata, Storage, StorageRead, VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;

mod query;

mod tasks {
    use super::{Files, Provider, open_storage, publish};
    use crate::{
        markdown::MarkdownParser,
        service::{CommandErrorKind, EmbeddingExecutor, EmbeddingService, Storage},
    };
    use rusqlite::Connection;
    use std::sync::{Arc, atomic::Ordering};
    use tempfile::TempDir;

    #[test]
    fn readiness_uses_durable_work_without_http_lock_or_retained_read() {
        // Given: isolated configuration, storage and controlled provider behavior.
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        let mut files = Files::new(0);
        publish(&mut storage, &mut files, "a.md", "ready token");
        let provider = Arc::new(Provider::new());
        let mut service = EmbeddingService::new(
            storage,
            files,
            MarkdownParser,
            Arc::<Provider>::clone(&provider),
        );
        // When: invoke the service operation.
        assert!(service.has_work().unwrap());
        // Then: no document/provider work; the read is gone before another unit starts.
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(service.files.reads.borrow().is_empty());
        drop(service.storage.begin_cache_write().unwrap());
    }

    #[test]
    fn readiness_preserves_storage_failure_as_a_safe_typed_error() {
        // Given: isolated configuration, storage and controlled provider behavior.
        let root = TempDir::new().unwrap();
        let provider = Arc::new(Provider::new());
        let mut service = EmbeddingService::new(
            open_storage(&root),
            Files::new(0),
            MarkdownParser,
            Arc::<Provider>::clone(&provider),
        );
        assert!(!service.has_work().unwrap());
        Connection::open(root.path().join("cache"))
            .unwrap()
            .execute_batch("ALTER TABLE nodes RENAME TO private_missing_nodes")
            .unwrap();
        // When: durable-read failure must not look like successful no-work.
        let error = service.has_work().unwrap_err();
        // Then: the result and persisted state match the scenario.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "embedding operation failed");
        assert!(!format!("{error:?}").contains("private_missing_nodes"));
    }
}

struct Files {
    config: RefCell<Option<Vec<u8>>>,
    documents: BTreeMap<String, Vec<u8>>,
    reads: RefCell<BTreeMap<String, usize>>,
    waits: RefCell<Vec<Duration>>,
    time: u64,
    config_reads: Cell<usize>,
    reject_config_reads: bool,
}
impl Files {
    fn new(retries: u64) -> Self {
        let config = serde_json::json!({"embedding_model": {
            "enabled": true, "provider": "ollama", "model": "m",
            "parameters": {"base_url": "http://localhost/api"}, "max_retries": retries,
        }});
        Self {
            config: RefCell::new(Some(serde_json::to_vec(&config).unwrap())),
            documents: BTreeMap::new(),
            reads: RefCell::default(),
            waits: RefCell::default(),
            time: 17,
            config_reads: Cell::new(0),
            reject_config_reads: false,
        }
    }
    fn get_configured(&self) -> EmbeddingConfig {
        Configuration::parse(self.config.borrow().as_ref().unwrap())
            .unwrap()
            .get_embedding_config()
            .unwrap()
            .clone()
    }
}
impl EmbeddingFiles for Files {
    type EmbedLock = ();
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        self.config_reads.set(self.config_reads.get() + 1);
        if self.reject_config_reads {
            return Err(EmbeddingError::new(EmbeddingErrorKind::Input));
        }
        Ok(self.config.borrow().clone())
    }
    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>> {
        *self
            .reads
            .borrow_mut()
            .entry(path.as_str().into())
            .or_default() += 1;
        Ok(self.documents.get(path.as_str()).cloned())
    }
    fn try_lock_embed(&self) -> EmbeddingResult<Option<()>> {
        Ok(Some(()))
    }
    fn get_time_secs(&self) -> EmbeddingResult<u64> {
        Ok(self.time)
    }
    fn wait(&self, delay: Duration) -> EmbeddingResult<()> {
        self.waits.borrow_mut().push(delay);
        Ok(())
    }
}
struct Provider {
    calls: AtomicUsize,
    invalid_config: bool,
    failure: Mutex<Option<EmbeddingErrorKind>>,
    vectors: Vec<Vec<f32>>,
    requests: Mutex<Vec<(Sha256, String, Option<NonZeroUsize>)>>,
}
impl Provider {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            invalid_config: false,
            failure: Mutex::new(None),
            vectors: vec![vec![1., 2., 3.]],
            requests: Mutex::default(),
        }
    }
}
impl EmbeddingProvider for Provider {
    fn validate_config(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
        if self.invalid_config {
            Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
        } else {
            Ok(())
        }
    }
    fn embed(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .unwrap()
            .push((config.get_identity(), texts[0].into(), dimensions));
        match *self.failure.lock().unwrap() {
            Some(kind) => Err(EmbeddingError::new(kind)),
            None => Ok(self.vectors.clone()),
        }
    }
}
#[test]
fn configuration_is_reread_and_provider_validation_precedes_identity_reset() {
    // Given: isolated configuration, storage and controlled provider behavior.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let old = Sha256::digest(b"old identity");
    storage.reset_vectors(None, &old).unwrap();
    let mut provider = Provider::new();
    provider.invalid_config = true;
    let provider = Arc::new(provider);
    let mut service = EmbeddingService::new(
        storage,
        Files::new(0),
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When / Then
    assert!(matches!(
        service.read_configuration().unwrap(),
        ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig)
    ));
    assert_eq!(
        service.storage.begin_read().unwrap().get_config_identity(),
        Some(old)
    );
    *service.files.config.borrow_mut() = None;
    assert!(matches!(
        service.read_configuration().unwrap(),
        ConfigState::Missing
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn skipped_ids_are_never_reread_when_processed_work_restarts_selection() {
    // Given: isolated configuration, storage and controlled provider behavior.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    publish(&mut storage, &mut files, "a.md", "shared");
    publish(&mut storage, &mut files, "b.md", "shared");
    publish(&mut storage, &mut files, "c.md", "different");
    files.documents.insert("a.md".into(), b"unmarked".to_vec());
    let provider = Arc::new(Provider::new());
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: invoke the service operation.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: the result and persisted state match the scenario.
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *service.files.reads.borrow(),
        BTreeMap::from([("a.md".into(), 1), ("b.md".into(), 1), ("c.md".into(), 1)])
    );
}

#[test]
fn credential_failures_use_four_durable_attempts_and_three_one_second_waits() {
    // Given: isolated configuration, storage and controlled provider behavior.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(3);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let provider = Provider::new();
    *provider.failure.lock().unwrap() = Some(EmbeddingErrorKind::Credentials);
    let provider = Arc::new(provider);
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: invoke the service operation.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: the result and persisted state match the scenario.
    assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    assert_eq!(*service.files.waits.borrow(), vec![RETRY_DELAY; 3]);
    let ConfigState::Ready(config) = service.read_configuration().unwrap() else {
        panic!()
    };
    let VectorOutcome::Current(Some(failure)) = service
        .storage
        .begin_read()
        .unwrap()
        .read_failure(&config.get_identity(), &hash)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failure.attempts, 4);
    assert_eq!(failure.error.unwrap(), "embedding credentials unavailable");
}

#[test]
fn rejected_vector_input_drops_poisoned_unit_before_recording_failure() {
    // Given: a deliberately faulty provider bypasses its native size contract.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let documents = files.documents.clone();
    let revision = storage.begin_read().unwrap().get_revision();
    let mut provider = Provider::new();
    provider.vectors = vec![vec![1.]];
    let provider = Arc::new(provider);
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: invoke the service operation.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: storage rejection rolled back, then a fresh unit persisted the error.
    let ConfigState::Ready(config) = service.read_configuration().unwrap() else {
        panic!()
    };
    let mut read = service.storage.begin_read().unwrap();
    let VectorOutcome::Current(Some(failure)) =
        read.read_failure(&config.get_identity(), &hash).unwrap()
    else {
        panic!()
    };
    assert_eq!(failure.attempts, 1);
    assert_eq!(failure.error.unwrap(), "invalid embedding response");
    assert_eq!(
        read.read_vector(&config.get_identity(), &hash).unwrap(),
        VectorOutcome::Current(None)
    );
    assert_eq!(read.get_revision(), revision);
    assert_eq!(service.files.documents, documents);
    assert_eq!(read.match_text("knowledge").unwrap().len(), 1);
}

mod process;

fn open_storage(root: &TempDir) -> SqliteStorage {
    SqliteStorage::open(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn publish(storage: &mut SqliteStorage, files: &mut Files, path: &str, body: &str) -> Sha256 {
    let bytes = format!("<!-- letopis: {{\"created_at\":1}} -->\n{body}").into_bytes();
    let mut parsed = parse(&RepoPath::parse(path).unwrap(), &bytes)
        .unwrap()
        .unwrap();
    let element = parsed.elements.remove(0);
    let mut unit = storage.begin_cache_write().unwrap();
    unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
        id: element.element.id.clone(),
        uri: element.element.id,
        kind: ElementKind::Vertex,
        hash: element.element.hash,
        created_at: 1,
        source_snapshot: None,
        lines: element.element.lines,
        search_hash: element.search_hash,
    }))
    .unwrap();
    unit.replace_text(&KnowledgeId::parse(path).unwrap(), &element.search_text)
        .unwrap();
    unit.commit().unwrap();
    files.documents.insert(path.into(), bytes);
    element.search_hash
}

pub(super) fn reach_checkpoint(stage: &str) {
    process::reach_checkpoint(stage);
}
