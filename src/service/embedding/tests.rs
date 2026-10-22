use super::{EmbeddingService, RETRY_DELAY};
use crate::{
    domain::{ElementKind, KnowledgeId, RepoPath, Sha256},
    markdown::{MarkdownParser, parse},
    service::{
        CacheWrite, ConfigState, DrainOutcome, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind,
        EmbeddingFiles, EmbeddingProvider, EmbeddingResult, Embeddings, KnowledgeMetadata,
        NodeMetadata, QueryEmbedding, Storage, StorageRead, VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::NonZeroUsize,
    time::Duration,
};
use tempfile::TempDir;

struct Files {
    config: RefCell<Option<Vec<u8>>>,
    documents: BTreeMap<String, Vec<u8>>,
    reads: RefCell<BTreeMap<String, usize>>,
    waits: RefCell<Vec<Duration>>,
    time: u64,
}
impl Files {
    fn new(retries: u64) -> Self {
        let config = serde_json::json!({
            "provider": "ollama", "model": "m",
            "parameters": {"base_url": "http://localhost/api"}, "max_retries": retries,
        });
        Self {
            config: RefCell::new(Some(serde_json::to_vec(&config).unwrap())),
            documents: BTreeMap::new(),
            reads: RefCell::default(),
            waits: RefCell::default(),
            time: 17,
        }
    }
}
impl EmbeddingFiles for Files {
    type EmbedLock = ();
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
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
    calls: Cell<usize>,
    invalid_config: bool,
    failure: Option<EmbeddingErrorKind>,
    vectors: Vec<Vec<f32>>,
}
impl Provider {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            invalid_config: false,
            failure: None,
            vectors: vec![vec![1., 2., 3.]],
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
        _: &EmbeddingConfig,
        _: &[&str],
        _: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.calls.set(self.calls.get() + 1);
        match self.failure {
            Some(kind) => Err(EmbeddingError::new(kind)),
            None => Ok(self.vectors.clone()),
        }
    }
}
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

#[test]
fn configuration_is_reread_and_provider_validation_precedes_identity_reset() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let old = Sha256::digest(b"old identity");
    storage.reset_vectors(&old).unwrap();
    let mut provider = Provider::new();
    provider.invalid_config = true;
    let mut service = EmbeddingService::new(storage, Files::new(0), MarkdownParser, provider);
    // Act and assert
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
    assert_eq!(service.provider.calls.get(), 0);
}

#[test]
fn skipped_ids_are_never_reread_when_processed_work_restarts_selection() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    publish(&mut storage, &mut files, "a.md", "shared");
    publish(&mut storage, &mut files, "b.md", "shared");
    publish(&mut storage, &mut files, "c.md", "different");
    files.documents.insert("a.md".into(), b"unmarked".to_vec());
    let mut service = EmbeddingService::new(storage, files, MarkdownParser, Provider::new());
    // Act
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Assert
    assert_eq!(service.provider.calls.get(), 2);
    assert_eq!(
        *service.files.reads.borrow(),
        BTreeMap::from([("a.md".into(), 1), ("b.md".into(), 1), ("c.md".into(), 1)])
    );
}

#[test]
fn credential_failures_use_four_durable_attempts_and_three_one_second_waits() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(3);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let mut provider = Provider::new();
    provider.failure = Some(EmbeddingErrorKind::Credentials);
    let mut service = EmbeddingService::new(storage, files, MarkdownParser, provider);
    // Act
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Assert
    assert_eq!(service.provider.calls.get(), 4);
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
fn service_defensively_rejects_provider_cardinality_and_nonfinite_vectors() {
    for vectors in [
        vec![],
        vec![vec![]],
        vec![vec![f32::NAN, 1.]],
        vec![vec![1., 2.]; 2],
    ] {
        // Arrange
        let root = TempDir::new().unwrap();
        let mut provider = Provider::new();
        provider.vectors = vectors;
        let mut service =
            EmbeddingService::new(open_storage(&root), Files::new(0), MarkdownParser, provider);
        // Act
        assert_eq!(
            service.embed_query("query").unwrap(),
            QueryEmbedding::Unavailable
        );
        // Assert
        assert_eq!(service.provider.calls.get(), 1);
        assert_eq!(
            service
                .storage
                .begin_read()
                .unwrap()
                .read_query_error()
                .unwrap()
                .unwrap()
                .error,
            "invalid embedding response"
        );
    }
}

#[test]
fn unsupported_query_timestamp_is_an_internal_error_without_truncation() {
    let root = TempDir::new().unwrap();
    let mut files = Files::new(0);
    files.time = u64::MAX;
    let mut provider = Provider::new();
    provider.failure = Some(EmbeddingErrorKind::Connection);
    let mut service = EmbeddingService::new(open_storage(&root), files, MarkdownParser, provider);
    let error = service.embed_query("query").unwrap_err();
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
    assert_eq!(
        service
            .storage
            .begin_read()
            .unwrap()
            .read_query_error()
            .unwrap(),
        None
    );
}

#[test]
fn rejected_vector_input_drops_poisoned_unit_before_recording_failure() {
    // Arrange: a deliberately faulty provider bypasses its native size contract.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let documents = files.documents.clone();
    let revision = storage.begin_read().unwrap().get_revision();
    let mut provider = Provider::new();
    provider.vectors = vec![vec![1.]];
    let mut service = EmbeddingService::new(storage, files, MarkdownParser, provider);
    // Act
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Assert: storage rejection rolled back, then a fresh unit persisted the error.
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
