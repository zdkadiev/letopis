use crate::support::storage::get_current;
use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
use support::reading::ReadingRepo;
use support::storage::{Databases, open_storage};
mod support;
use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, RepoPath, Sha256},
    markdown::file_parse,
    open_filesystem,
    service::{
        ConfigState, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind, EmbeddingFailure,
        FileLocker, FileRow, KnowledgeMetadata, NodeMetadata, QueryEmbedding,
        QueryEmbeddingRequest, QueryError, VectorOutcome,
    },
    wire_embedder,
};
use rusqlite::{Connection, types::Value};
use std::{fs, num::NonZeroUsize, thread};
use support::embedding::{FixtureReply, HttpFixture, ResponseBarrier};
use tempfile::TempDir;

#[derive(Debug, PartialEq)]
struct DiscoveryEmbeddingState {
    identity: Option<Sha256>,
    vector: VectorOutcome<Option<Vec<f32>>>,
    failure: VectorOutcome<Option<EmbeddingFailure>>,
    query_error: Option<QueryError>,
}

struct Fixture {
    root: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: TempDir::new().unwrap(),
        }
    }
    fn open_storage(&self) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
        open_storage(
            &self.root.path().join("cache"),
            &self.root.path().join("vectors"),
        )
        .unwrap()
    }
    fn open_service(&self) -> impl Embedder + use<> {
        open_service_at(self.root.path())
    }
    fn write_config(&self, url: &str, retries: u64) -> EmbeddingConfig {
        self.write_raw_config(
            &serde_json::json!({"provider":"ollama", "model":"fixture",
            "parameters":{"base_url":url}, "max_retries":retries})
            .to_string(),
        )
    }
    fn write_disabled_config(&self, url: &str, retries: u64) {
        let raw = serde_json::to_vec(&serde_json::json!({"embedding_model": {
            "enabled": false, "provider": "ollama", "model": "fixture",
            "parameters": {"base_url": url}, "max_retries": retries,
        }}))
        .unwrap();
        fs::write(self.root.path().join(".letopis/config.json"), raw).unwrap();
    }
    fn write_raw_config(&self, raw: &str) -> EmbeddingConfig {
        let configured: serde_json::Value = serde_json::from_str(raw).unwrap();
        let file = serde_json::json!({"embedding_model": {
            "enabled": true,
            "provider": configured["provider"],
            "model": configured["model"],
            "parameters": configured["parameters"],
            "max_retries": configured["max_retries"],
        }});
        fs::create_dir_all(self.root.path().join(".letopis")).unwrap();
        fs::write(
            self.root.path().join(".letopis/config.json"),
            serde_json::to_vec(&file).unwrap(),
        )
        .unwrap();
        EmbeddingConfig::parse(raw.as_bytes()).unwrap()
    }
    fn publish(&self, path: &str, body: &str) -> Sha256 {
        let text = support::build_whole_file(body);
        fs::write(self.root.path().join(path), &text).unwrap();
        let parsed = file_parse(&RepoPath::parse(path).unwrap(), text.as_bytes())
            .unwrap()
            .unwrap();
        let element = parsed.elements.into_iter().next().unwrap();
        let metadata = &element.element;
        let storage = self.open_storage();
        storage
            .cache
            .with_write_tx(|| {
                let unit = &storage.cache;
                unit.edges_delete_by_source_id(&metadata.id).unwrap();
                unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
                    id: metadata.id.clone(),
                    uri: metadata.id.clone(),
                    kind: ElementKind::Vertex,
                    hash: Sha256::digest(text.as_bytes()),
                    created_at: 1,
                    source_snapshot: None,
                    lines: metadata.lines,
                    search_hash: element.search_hash,
                }))
                .unwrap();
                unit.replace_text(&metadata.id, &element.search_text)
                    .unwrap();
                unit.edges_save_by_source_id_and_target_id_and_type(
                    &Edge::new(
                        metadata.id.clone(),
                        EdgeTarget::Knowledge(KnowledgeId::parse("dangling.md").unwrap()),
                        EdgeType::parse("supports").unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();
                unit.files_upsert_by_path(&FileRow {
                    path: RepoPath::parse(path).unwrap(),
                    size: u64::try_from(text.len()).unwrap(),
                    mtime_ns: 0,
                    inode: 1,
                    hash: Sha256::digest(text.as_bytes()),
                    checked_at_ns: 0,
                    valid: true,
                    error: None,
                })
                .unwrap();

                unit.project_get_revision()
            })
            .unwrap();
        element.search_hash
    }
    fn get_failure_by_search_hash(
        &self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
    ) -> Option<EmbeddingFailure> {
        unwrap_current(
            {
                let scope_databases = self.open_storage();
                scope_databases
                    .cache
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(
                            scope_databases.cache.embed_failures_get_by_search_hash(
                                &config.get_identity(),
                                search_hash,
                            ),
                        )
                    })
                    .unwrap()
            }
            .unwrap(),
        )
    }
    fn get_vector_by_search_hash(
        &self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
    ) -> Option<Vec<f32>> {
        unwrap_current(
            {
                let scope_databases = self.open_storage();
                scope_databases
                    .cache
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(
                            scope_databases.cache.vector_keys_get_by_search_hash(
                                &config.get_identity(),
                                search_hash,
                            ),
                        )
                    })
                    .unwrap()
            }
            .unwrap(),
        )
    }
    fn get_query_error(&self) -> Option<QueryError> {
        {
            let scope_databases = self.open_storage();
            scope_databases
                .cache
                .with_read_tx(|| {
                    Ok::<_, StorageError>(scope_databases.cache.query_error_get_by_id())
                })
                .unwrap()
        }
        .unwrap()
    }
    fn capture_corpus(&self) -> Corpus {
        let db = Connection::open(self.root.path().join("cache")).unwrap();
        let tables = [
            "nodes",
            "edges",
            "files",
            "project",
            "text_index_docsize",
            "text_index_data",
            "text_index_idx",
            "text_index_config",
        ];
        let rows = tables
            .into_iter()
            .map(|table| {
                let mut query = db
                    .prepare(&format!("select * from {table} order by 1"))
                    .unwrap();
                let columns = query.column_count();
                query
                    .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
                    .unwrap()
                    .collect::<Result<Vec<Vec<Value>>, _>>()
                    .unwrap()
            })
            .collect();
        let mut files: Vec<_> = fs::read_dir(self.root.path())
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_type().unwrap().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
            .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
            .collect();
        files.sort();
        let storage = self.open_storage();
        storage
            .cache
            .with_read_tx(|| {
                Ok::<_, StorageError>({
                    let read = &storage.cache;
                    let matches = read.text_index_get_by_expression("knowledge").unwrap();
                    Corpus {
                        rows,
                        files,
                        matches: matches.into_iter().map(|item| item.id).collect(),
                        revision: read.project_get_by_id().unwrap(),
                    }
                })
            })
            .unwrap()
    }
}
#[derive(Debug, PartialEq)]
struct Corpus {
    rows: Vec<Vec<Vec<Value>>>,
    files: Vec<(std::ffi::OsString, Vec<u8>)>,
    matches: Vec<KnowledgeId>,
    revision: u64,
}
struct Operation<T> {
    barrier: ResponseBarrier,
    worker: Option<thread::JoinHandle<T>>,
}
impl<T: Send + 'static> Operation<T> {
    fn spawn(barrier: ResponseBarrier, action: impl FnOnce() -> T + Send + 'static) -> Self {
        Self {
            barrier,
            worker: Some(thread::spawn(action)),
        }
    }
    fn join(mut self) -> thread::Result<T> {
        self.barrier.release();
        self.worker.take().unwrap().join()
    }
}
impl<T> Drop for Operation<T> {
    fn drop(&mut self) {
        self.barrier.release();
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                assert!(result.is_ok(), "service worker panicked");
            }
        }
    }
}

#[test]
fn discovery_only_changes_preserve_embedding_state() {
    use letopis::service::Refresher;
    // Given: two owners share a seeded vector, durable failure and query diagnostic.
    let repo = ReadingRepo::new();
    let server = start_success();
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    for path in ["a.md", "b.md"] {
        repo.write_knowledge(path, "StableNeedle");
    }
    let mut refresher = letopis::wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh(&[]).unwrap();
    let hash = seed_discovery_embedding_state(&repo, &config);
    let before = get_discovery_embedding_state(&repo, &config, &hash);
    assert_eq!(
        before.vector,
        VectorOutcome::Current(Some(vec![1., 2., 3.]))
    );
    assert_eq!(
        before.failure,
        VectorOutcome::Current(Some(EmbeddingFailure {
            search_hash: hash,
            attempts: 1,
            error: Some("preserved failure".into())
        }))
    );
    // When: discovery toggles without changing membership and later removes one shared owner.
    for respect_gitignore in [false, true] {
        let mut raw: serde_json::Value = serde_json::from_slice(
            &fs::read(repo.get_root().join(".letopis/config.json")).unwrap(),
        )
        .unwrap();
        raw["discovery"] = serde_json::json!({"respect_gitignore":respect_gitignore});
        repo.write(".letopis/config.json", &serde_json::to_vec(&raw).unwrap());
        assert!(!refresher.refresh(&[]).unwrap().is_published);
        let mut embedder = letopis::wire_embedding_executor(repo.get_root()).unwrap();
        assert!(matches!(
            embedder.get_configuration().unwrap(),
            ConfigState::Ready(_)
        ));
        // Then: readiness preserves the existing vector, attempts, identity and query error.
        assert_eq!(get_discovery_embedding_state(&repo, &config, &hash), before);
    }
    // When: the next refresh excludes just one of the shared owners.
    repo.write(".letopis/ignore", b"b.md\n");
    assert_eq!(refresher.refresh(&[]).unwrap().files_removed, 1);
    // Then: the surviving hash keeps its state and enumeration never calls HTTP.
    assert_eq!(get_discovery_embedding_state(&repo, &config, &hash), before);
    assert!(server.take_requests().is_empty());
}

#[test]
fn missing_and_invalid_configuration_preserve_corpus_and_prior_vectors() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    server.take_requests();
    // When / Then
    for bytes in [None, Some(b"invalid".as_slice())] {
        let path = fixture.root.path().join(".letopis/config.json");
        match bytes {
            None => fs::remove_file(path).unwrap(),
            Some(bytes) => fs::write(path, bytes).unwrap(),
        }
        let before = fixture.capture_corpus();
        assert!(matches!(
            service.get_configuration().unwrap(),
            ConfigState::Missing | ConfigState::Invalid(_)
        ));
        assert_eq!(service.drain().unwrap(), DrainOutcome::Unavailable);
        assert_eq!(
            service.embed_query("query").unwrap(),
            QueryEmbedding::Unavailable
        );
        assert_eq!(
            fixture.get_vector_by_search_hash(&config, &hash),
            Some(vec![1., 2., 3.])
        );
        assert_eq!(fixture.capture_corpus(), before);
        assert_eq!(server.take_requests().len(), 0);
    }
}

#[test]
fn disabled_configuration_preserves_vectors_attempts_and_skips_execution() {
    // Given: one stored vector, one exhausted hash and a valid disabled document.
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let exhausted = fixture.publish("b.md", "other knowledge");
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    exhaust_attempt(&fixture, &config, &exhausted);
    server.take_requests();
    fixture.write_disabled_config(&server.get_base_url(), 0);
    let before = fixture.capture_corpus();

    // When
    let mut service = fixture.open_service();
    assert!(matches!(
        service.get_configuration().unwrap(),
        ConfigState::Disabled
    ));
    assert_eq!(service.drain().unwrap(), DrainOutcome::Unavailable);
    assert_eq!(
        service.embed_query("query").unwrap(),
        QueryEmbedding::Unavailable
    );

    // Then: both the vector and retry state survive without inference or reset.
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &exhausted)
            .unwrap()
            .attempts,
        1
    );
    assert_eq!(fixture.capture_corpus(), before);
    assert!(server.take_requests().is_empty());

    // When: the same settings are re-enabled.
    let enabled = fixture.write_config(&server.get_base_url(), 0);
    assert_eq!(enabled.get_identity(), config.get_identity());
    assert!(matches!(
        fixture.open_service().get_configuration().unwrap(),
        ConfigState::Ready(_)
    ));

    // Then: the exact stored execution state remains available.
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &exhausted)
            .unwrap()
            .attempts,
        1
    );
    assert!(server.take_requests().is_empty());
}

#[test]
fn unreadable_configuration_returns_safe_invalid_state() {
    // Given: the embedding assembly pins its directory before a nonregular config is installed.
    let fixture = Fixture::new();
    let mut service = fixture.open_service();
    fs::create_dir(fixture.root.path().join(".letopis/config.json")).unwrap();
    let before = fixture.capture_corpus();
    // When / Then: the strict config reader returns invalid input and preserves the corpus.
    assert!(matches!(
        service.get_configuration().unwrap(),
        ConfigState::Invalid(EmbeddingErrorKind::Input)
    ));
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn changed_configuration_resets_vectors_but_equal_identity_keeps_them() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    service.drain().unwrap();
    let before = fixture.capture_corpus();
    // When / Then
    assert!(matches!(
        service.get_configuration().unwrap(),
        ConfigState::Ready(_)
    ));
    assert!(fixture.get_vector_by_search_hash(&config, &hash).is_some());
    let next = fixture.write_config(&server.get_base_url(), 1);
    service.get_configuration().unwrap();
    assert_eq!(fixture.get_vector_by_search_hash(&next, &hash), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn stale_first_same_hash_does_not_starve_valid_second_representative() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fixture.publish("b.md", "knowledge");
    fs::write(
        fixture.root.path().join("a.md"),
        support::build_whole_file("edited"),
    )
    .unwrap();
    let before = fixture.capture_corpus();
    // When / Then
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(server.take_requests().len(), 1);
    assert!(fixture.get_vector_by_search_hash(&config, &hash).is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn stale_and_missing_elements_skip_without_reservations_or_requests() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let other = fixture.publish("b.md", "other");
    fs::write(fixture.root.path().join("a.md"), "unmarked").unwrap();
    fs::remove_file(fixture.root.path().join("b.md")).unwrap();
    let before = fixture.capture_corpus();
    // When / Then
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.get_failure_by_search_hash(&config, &hash), None);
    assert_eq!(fixture.get_failure_by_search_hash(&config, &other), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn retries_are_additional_and_exhaustion_survives_restart() {
    // Given
    for (retries, attempts) in [(0, 1), (3, 4)] {
        let fixture = Fixture::new();
        let server = start_failure();
        let config = fixture.write_config(&server.get_base_url(), retries);
        let hash = fixture.publish("a.md", "knowledge");
        let before = fixture.capture_corpus();
        // When / Then
        assert_eq!(
            fixture.open_service().drain().unwrap(),
            DrainOutcome::Complete
        );
        assert_eq!(server.take_requests().len(), attempts);
        let failed = fixture.get_failure_by_search_hash(&config, &hash).unwrap();
        assert_eq!(failed.attempts, u64::try_from(attempts).unwrap());
        assert!(!failed.error.unwrap().contains("private"));
        assert_eq!(fixture.capture_corpus(), before);
        assert_eq!(
            fixture.open_service().drain().unwrap(),
            DrainOutcome::Complete
        );
        assert_eq!(server.take_requests().len(), 0);
        assert_eq!(fixture.capture_corpus(), before);
    }
}

#[test]
fn malformed_document_records_a_durable_failure_without_http() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fs::write(fixture.root.path().join("a.md"), [0xff]).unwrap();
    let before = fixture.capture_corpus();
    // When
    fixture.open_service().drain().unwrap();
    // Then
    assert!(
        fixture
            .get_failure_by_search_hash(&config, &hash)
            .unwrap()
            .attempts
            >= 1
    );
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn document_read_failure_uses_the_same_durable_budget() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fs::remove_file(fixture.root.path().join("a.md")).unwrap();
    fs::create_dir(fixture.root.path().join("a.md")).unwrap();
    let before = fixture.capture_corpus();
    // When
    fixture.open_service().drain().unwrap();
    // Then
    assert!(
        fixture
            .get_failure_by_search_hash(&config, &hash)
            .unwrap()
            .attempts
            >= 1
    );
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn configured_dimensions_reject_wrong_size_without_graph_changes() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_raw_config(
        &serde_json::json!({"provider":"ollama", "model":"fixture",
        "parameters":{"base_url":server.get_base_url(), "dimensions":4}, "max_retries":0})
        .to_string(),
    );
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    // When
    fixture.open_service().drain().unwrap();
    // Then
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &hash)
            .unwrap()
            .attempts,
        1
    );
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn query_failure_is_single_attempt_without_document_reservation_or_vector() {
    // Given
    let fixture = Fixture::new();
    let server = start_failure();
    let config = fixture.write_config(&server.get_base_url(), 3);
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    // When / Then
    assert_eq!(
        fixture.open_service().embed_query("query").unwrap(),
        QueryEmbedding::Unavailable
    );
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(fixture.get_failure_by_search_hash(&config, &hash), None);
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    assert!(fixture.get_query_error().is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn query_success_clears_error_without_caching_or_document_attempts() {
    // Given: a persisted query error and successful query provider.
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let storage = fixture.open_storage();
    storage
        .vectors
        .reset_vectors(None, &config.get_identity())
        .unwrap();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.query_error_upsert_by_id(1, "old failure").unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = fixture.capture_corpus();
    // When / Then: the query is embedded successfully; the error clears without caching or consuming document attempts.
    assert_eq!(
        fixture.open_service().embed_query("query").unwrap(),
        QueryEmbedding::Available {
            identity: config.get_identity(),
            vector: vec![1., 2., 3.]
        }
    );
    assert_eq!(fixture.get_query_error(), None);
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    assert_eq!(fixture.get_failure_by_search_hash(&config, &hash), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn persistent_lock_prevents_another_drain() {
    // Given: one active drain ownership guard.
    let fixture = Fixture::new();
    let server = start_success();
    fixture.write_config(&server.get_base_url(), 0);
    fixture.publish("a.md", "knowledge");
    let files = open_filesystem(fixture.root.path()).unwrap();
    let guard = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    // When / Then: another embedding service attempts to drain; the second drain performs no provider work.
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::AlreadyRunning
    );
    assert!(server.take_requests().is_empty());
    drop(guard);
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
}

#[test]
fn reservation_precedes_http_and_new_work_is_found_after_compute() {
    // Given
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let old = fixture.publish("z.md", "old knowledge");
    let service_root = fixture.root.path().to_path_buf();
    let open_worker_service = move || open_service_at(&service_root);
    // When
    let worker = Operation::spawn(barrier.clone(), move || {
        open_worker_service().drain().unwrap()
    });
    barrier.wait_until_entered();
    // Then
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &old)
            .unwrap()
            .attempts,
        1
    );
    assert_eq!(fixture.get_vector_by_search_hash(&config, &old), None);
    let new = fixture.publish("a.md", "new knowledge");
    let before = fixture.capture_corpus();
    barrier.release();
    assert_eq!(worker.join().unwrap(), DrainOutcome::Complete);
    assert_eq!(server.take_requests().len(), 2);
    assert!(fixture.get_vector_by_search_hash(&config, &old).is_some());
    assert!(fixture.get_vector_by_search_hash(&config, &new).is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn edit_during_compute_stores_captured_hash_and_keeps_new_hash_pending() {
    // Given: knowledge edited while its embedding request is in flight.
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let old = fixture.publish("a.md", "old knowledge");
    let service_root = fixture.root.path().to_path_buf();
    let open_worker_service = move || open_service_at(&service_root);
    let worker = Operation::spawn(barrier.clone(), move || {
        open_worker_service().drain().unwrap()
    });
    barrier.wait_until_entered();
    fs::write(
        fixture.root.path().join("a.md"),
        support::build_whole_file("new knowledge"),
    )
    .unwrap();
    let before = fixture.capture_corpus();
    // When: the captured embedding result returns.
    barrier.release();
    // Then: only the captured hash is stored and new text stays pending.
    assert_eq!(worker.join().unwrap(), DrainOutcome::Complete);
    assert!(fixture.get_vector_by_search_hash(&config, &old).is_some());
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(fixture.capture_corpus(), before);
    let new = fixture.publish("a.md", "new knowledge");
    assert_eq!(fixture.get_vector_by_search_hash(&config, &new), None);
    assert!(
        unwrap_current(
            {
                let scope_databases = fixture.open_storage();
                scope_databases
                    .cache
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(
                            scope_databases
                                .cache
                                .nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
                                    &config.get_identity(),
                                    1,
                                    None,
                                ),
                        )
                    })
                    .unwrap()
            }
            .unwrap()
        )
        .is_some()
    );
}

#[test]
fn reset_during_http_discards_old_result_without_old_cleanup() {
    // Given: a model reset while an embedding request is in flight.
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let service_root = fixture.root.path().to_path_buf();
    let open_worker_service = move || open_service_at(&service_root);
    let worker = Operation::spawn(barrier.clone(), move || {
        open_worker_service().drain().unwrap()
    });
    barrier.wait_until_entered();
    let next = fixture.write_config(&server.get_base_url(), 1);
    fixture.open_service().get_configuration().unwrap();
    let storage = fixture.open_storage();
    storage
        .vectors
        .with_write_tx(&next.get_identity(), || {
            let unit = &storage.vectors;
            unit.store_vector(&hash, &[1., 2., 3.], 3).unwrap();
            unit.query_error_upsert_by_id(7, "current failure").unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = fixture.capture_corpus();
    // When: the obsolete response arrives.
    barrier.release();
    // Then: the result is discarded without cleaning up current vectors.
    assert_eq!(worker.join().unwrap(), DrainOutcome::ConfigChanged);
    assert!(fixture.get_vector_by_search_hash(&next, &hash).is_some());
    assert_eq!(fixture.get_query_error().unwrap().error, "current failure");
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn late_old_query_success_or_error_cannot_replace_new_query_state() {
    // Given
    for (body, new_success) in [
        (r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#, false),
        ("malformed response", false),
        ("malformed response", true),
    ] {
        let fixture = Fixture::new();
        let (old_server, barrier) = start_paused(body);
        fixture.write_config(&old_server.get_base_url(), 0);
        fixture.publish("a.md", "knowledge");
        let service_root = fixture.root.path().to_path_buf();
        let open_worker_old_service = move || open_service_at(&service_root);
        let worker = Operation::spawn(barrier.clone(), move || {
            open_worker_old_service().embed_query("query").unwrap()
        });
        barrier.wait_until_entered();
        let new_server = if new_success {
            start_success()
        } else {
            start_failure()
        };
        fixture.write_config(&new_server.get_base_url(), 0);
        // When
        let outcome = fixture.open_service().embed_query("new query").unwrap();
        // Then
        assert_eq!(
            matches!(outcome, QueryEmbedding::Available { .. }),
            new_success
        );
        let error = fixture.get_query_error();
        let before = fixture.capture_corpus();
        barrier.release();
        assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
        assert_eq!(fixture.get_query_error(), error);
        assert_eq!(fixture.capture_corpus(), before);
    }
}

#[test]
fn query_rechecks_dimension_established_during_http() {
    // Given: native dimensions established during a query request.
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let service_root = fixture.root.path().to_path_buf();
    let open_worker_service = move || open_service_at(&service_root);
    let worker = Operation::spawn(barrier.clone(), move || {
        open_worker_service().embed_query("query").unwrap()
    });
    barrier.wait_until_entered();
    let storage = fixture.open_storage();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.store_vector(&hash, &[1., 2., 3., 4.], 4).unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = fixture.capture_corpus();
    // When: the query provider response returns.
    barrier.release();
    // Then: the established dimensions are checked again.
    assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
    assert!(fixture.get_query_error().is_some());
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3., 4.])
    );
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn cleanup_retains_skipped_and_exhausted_references_and_removes_orphans() {
    // Given: referenced, skipped, exhausted and orphan embedding rows.
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 1);
    let skipped = fixture.publish("a.md", "knowledge");
    let exhausted = fixture.publish("b.md", "exhausted");
    let known = fixture.publish("c.md", "known");
    fs::write(fixture.root.path().join("a.md"), "unmarked").unwrap();
    let storage = fixture.open_storage();
    storage
        .vectors
        .reset_vectors(None, &config.get_identity())
        .unwrap();
    let orphan = Sha256::digest(b"orphan");
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.embed_failures_upsert_by_search_hash_and_attempts(&exhausted, 2)
                .unwrap();
            unit.embed_failures_upsert_by_search_hash_and_attempts(&exhausted, 2)
                .unwrap();
            unit.embed_failures_upsert_by_search_hash_and_attempts(&skipped, 2)
                .unwrap();
            unit.embed_failures_update_by_search_hash(&skipped, "prior skipped failure")
                .unwrap();
            unit.embed_failures_upsert_by_search_hash_and_attempts(&orphan, 2)
                .unwrap();
            unit.store_vector(&known, &[1., 2., 3.], 3).unwrap();
            unit.embed_failures_update_by_search_hash(&exhausted, "failure")
                .unwrap();
            unit.store_vector(&orphan, &[1., 2., 3.], 3).unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = fixture.capture_corpus();
    // When / Then: the drain performs cleanup; referenced rows survive and orphans are removed.
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(fixture.get_vector_by_search_hash(&config, &orphan), None);
    let exhausted_failure = fixture
        .get_failure_by_search_hash(&config, &exhausted)
        .unwrap();
    let skipped_failure = fixture
        .get_failure_by_search_hash(&config, &skipped)
        .unwrap();
    assert_eq!(exhausted_failure.attempts, 2);
    assert_eq!(skipped_failure.attempts, 1);
    assert_eq!(fixture.get_failure_by_search_hash(&config, &orphan), None);
    assert!(fixture.get_vector_by_search_hash(&config, &known).is_some());
    let id = EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap());
    assert!(
        storage
            .cache
            .with_read_tx(|| StorageRead::nodes_get_by_uri(&storage.cache, &id))
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn vector_storage_failure_propagates_internal_error_without_false_provider_failure() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    let mut service = fixture.open_service();
    let db = Connection::open(fixture.root.path().join("vectors")).unwrap();
    db.execute_batch(
        "create trigger reject_key before insert on vector_keys
        begin select raise(abort, 'private storage payload'); end;",
    )
    .unwrap();
    // When
    let error = service.drain().unwrap_err();
    db.execute_batch("drop trigger reject_key;").unwrap();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("private"));
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    let failed = fixture.get_failure_by_search_hash(&config, &hash).unwrap();
    assert_eq!(failed.attempts, 1);
    assert_eq!(failed.error, None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn valid_shape_with_invalid_endpoint_does_not_reset_or_compute() {
    // Given
    let fixture = Fixture::new();
    let server = start_success();
    let prior = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fixture.open_service().drain().unwrap();
    server.take_requests();
    fixture.write_config("ftp://localhost/api", 0);
    let before = fixture.capture_corpus();
    // When / Then
    assert!(matches!(
        fixture.open_service().get_configuration().unwrap(),
        ConfigState::Invalid(_)
    ));
    assert!(fixture.get_vector_by_search_hash(&prior, &hash).is_some());
    assert!(server.take_requests().is_empty());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn captured_query_checks_identity_before_http_and_before_diagnostic_commit() {
    // Given
    // Missing identity and replacement before invocation both forbid HTTP.
    for replacement in [false, true] {
        let fixture = Fixture::new();
        let server = start_success();
        let captured = fixture.write_config(&server.get_base_url(), 0);
        if replacement {
            let next = fixture.write_config(&server.get_base_url(), 1);
            seed_query_error(&fixture, &next);
        }
        let error = fixture.get_query_error();
        // When
        let result = fixture
            .open_service()
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &captured,
                dimensions: None,
            })
            .unwrap();
        // Then
        assert_eq!(result, QueryEmbedding::Unavailable);
        assert!(server.take_requests().is_empty());
        assert_eq!(fixture.get_query_error(), error);
    }
}

#[test]
fn captured_query_preserves_replacement_diagnostics_after_http() {
    // Given
    // Replacement during success or failure must retain its own diagnostic.
    for body in [
        r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#,
        "malformed",
    ] {
        let fixture = Fixture::new();
        let (server, barrier) = start_paused(body);
        let captured = fixture.write_config(&server.get_base_url(), 0);
        fixture.open_service().get_configuration().unwrap();
        let service_root = fixture.root.path().to_path_buf();
        let open_worker_service = move || open_service_at(&service_root);
        let worker = Operation::spawn(barrier.clone(), move || {
            open_worker_service()
                .embed_query_with_config(QueryEmbeddingRequest {
                    text: "query",
                    config: &captured,
                    dimensions: None,
                })
                .unwrap()
        });
        barrier.wait_until_entered();
        let next = fixture.write_config(&server.get_base_url(), 1);
        seed_query_error(&fixture, &next);
        // When
        let error = fixture.get_query_error();
        barrier.release();
        // Then
        assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
        assert_eq!(server.take_requests().len(), 1);
        assert_eq!(fixture.get_query_error(), error);
    }
}

#[test]
fn captured_query_validates_pinned_and_newly_established_dimensions() {
    // Given
    // Pinned size must be checked before a document establishes the index size.
    let fixture = Fixture::new();
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture-alias","embeddings":[[1,2]]}"#.into(),
    });
    let captured = fixture.write_config(&server.get_base_url(), 0);
    fixture.open_service().get_configuration().unwrap();
    // When
    let result = fixture
        .open_service()
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "query",
            config: &captured,
            dimensions: NonZeroUsize::new(3),
        })
        .unwrap();
    // Then
    assert_eq!(result, QueryEmbedding::Unavailable);
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(
        fixture.get_query_error().unwrap().error,
        "invalid embedding response"
    );
}

#[test]
fn captured_query_rejects_dimensions_established_during_http() {
    // Given: captured query dimensions established while HTTP is pending.
    // First document vector can establish another size while inference is paused.
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let captured = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fixture.open_service().get_configuration().unwrap();
    let config = captured.clone();
    let service_root = fixture.root.path().to_path_buf();
    let open_worker_service = move || open_service_at(&service_root);
    // When: the captured query response returns.
    let worker = Operation::spawn(barrier.clone(), move || {
        open_worker_service()
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &captured,
                dimensions: None,
            })
            .unwrap()
    });
    barrier.wait_until_entered();
    let storage = fixture.open_storage();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.store_vector(&hash, &[1., 2., 3., 4.], 4).unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    barrier.release();
    // Then: the newly established native dimensions are enforced.
    assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3., 4.])
    );
    assert_eq!(
        fixture.get_query_error().unwrap().error,
        "invalid embedding response"
    );
}

#[test]
fn captured_query_diagnostic_write_failure_remains_an_error() {
    // Given
    for succeeds in [false, true] {
        let fixture = Fixture::new();
        let server = if succeeds {
            start_success()
        } else {
            start_failure()
        };
        let config = fixture.write_config(&server.get_base_url(), 0);
        seed_query_error(&fixture, &config);
        let mut service = fixture.open_service();
        let db = Connection::open(fixture.root.path().join("vectors")).unwrap();
        db.execute_batch(
            "create trigger reject_query_insert before insert on query_error
             begin select raise(abort, 'private diagnostic'); end;
             create trigger reject_query_delete before delete on query_error
             begin select raise(abort, 'private diagnostic'); end;",
        )
        .unwrap();
        // When
        let error = service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &config,
                dimensions: None,
            })
            .unwrap_err();
        db.execute_batch("drop trigger reject_query_insert; drop trigger reject_query_delete;")
            .unwrap();
        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
        assert!(!format!("{error:?}").contains("private"));
        assert_eq!(
            fixture.get_query_error().unwrap().error,
            "replacement diagnostic"
        );
        assert_eq!(server.take_requests().len(), 1);
    }
}

fn seed_query_error(fixture: &Fixture, config: &EmbeddingConfig) {
    let storage = fixture.open_storage();
    let observed = storage
        .cache
        .with_read_tx(|| Ok::<_, StorageError>(storage.cache.meta_get_by_id()))
        .unwrap()
        .unwrap();
    storage
        .vectors
        .reset_vectors(observed.as_ref(), &config.get_identity())
        .unwrap();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.query_error_upsert_by_id(7, "replacement diagnostic")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}

fn start_success() -> HttpFixture {
    HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#.into(),
    })
}

fn start_failure() -> HttpFixture {
    HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "private provider payload".into(),
    })
}

fn start_paused(body: &str) -> (HttpFixture, ResponseBarrier) {
    let barrier = ResponseBarrier::default();
    (
        HttpFixture::start(FixtureReply::Barrier {
            barrier: barrier.clone(),
            body: body.into(),
        }),
        barrier,
    )
}

fn exhaust_attempt(fixture: &Fixture, config: &EmbeddingConfig, search_hash: &Sha256) {
    let storage = fixture.open_storage();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            assert!(
                unit.embed_failures_upsert_by_search_hash_and_attempts(search_hash, 1)
                    .unwrap()
            );
            unit.embed_failures_update_by_search_hash(search_hash, "safe failure")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}

fn unwrap_current<T>(value: VectorOutcome<T>) -> T {
    match value {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected configuration change"),
    }
}

fn open_service_at(root: &std::path::Path) -> impl Embedder + use<> {
    let (vectors, cache) =
        letopis::open_storage(&root.join("cache"), &root.join("vectors")).unwrap();
    wire_embedder(root, std::rc::Rc::new(cache), std::rc::Rc::new(vectors)).unwrap()
}

fn seed_discovery_embedding_state(repo: &ReadingRepo, config: &EmbeddingConfig) -> Sha256 {
    repo.seed_vectors(config, &[("a.md", &[1., 2., 3.])]);
    let storage = repo.open_storage();
    let node = storage
        .cache
        .with_read_tx(|| {
            storage
                .cache
                .nodes_get_by_uri(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        })
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!("knowledge fixture")
    };
    let hash = node.search_hash;
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            storage
                .vectors
                .embed_failures_upsert_by_search_hash_and_attempts(&hash, 1)?;
            storage
                .vectors
                .embed_failures_update_by_search_hash(&hash, "preserved failure")?;
            storage
                .vectors
                .query_error_upsert_by_id(7, "preserved query")
        })
        .map(get_current)
        .unwrap();
    hash
}

fn get_discovery_embedding_state(
    repo: &ReadingRepo,
    config: &EmbeddingConfig,
    hash: &Sha256,
) -> DiscoveryEmbeddingState {
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| -> Result<DiscoveryEmbeddingState, StorageError> {
            Ok(DiscoveryEmbeddingState {
                identity: storage.cache.meta_get_by_id()?,
                vector: storage
                    .cache
                    .vector_keys_get_by_search_hash(&config.get_identity(), hash)?,
                failure: storage
                    .cache
                    .embed_failures_get_by_search_hash(&config.get_identity(), hash)?,
                query_error: storage.cache.query_error_get_by_id()?,
            })
        })
        .unwrap()
}
