mod support;
use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, RepoPath, Sha256},
    markdown::parse,
    open_embedding_files, open_storage,
    service::{
        CacheWrite, ConfigState, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind,
        EmbeddingFailure, EmbeddingFiles, FileRow, KnowledgeMetadata, NodeMetadata, QueryEmbedding,
        QueryEmbeddingRequest, QueryError, Storage, StorageRead, VectorOutcome, VectorWrite,
    },
    wire_embedder,
};
use rusqlite::{Connection, types::Value};
use std::{fs, num::NonZeroUsize, thread};
use support::embedding::{FixtureReply, HttpFixture, ResponseBarrier};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: TempDir::new().unwrap(),
        }
    }
    fn open_storage(&self) -> impl Storage + use<> {
        open_storage(
            &self.root.path().join("cache"),
            &self.root.path().join("vectors"),
        )
        .unwrap()
    }
    fn open_service(&self) -> impl Embedder + use<> {
        wire_embedder(self.root.path(), self.open_storage()).unwrap()
    }
    fn write_config(&self, url: &str, retries: u64) -> EmbeddingConfig {
        self.write_raw_config(
            &serde_json::json!({"provider":"ollama", "model":"fixture",
            "parameters":{"base_url":url}, "max_retries":retries})
            .to_string(),
        )
    }
    fn write_raw_config(&self, raw: &str) -> EmbeddingConfig {
        fs::create_dir_all(self.root.path().join(".letopis")).unwrap();
        fs::write(self.root.path().join(".letopis/config.json"), raw).unwrap();
        EmbeddingConfig::parse(raw.as_bytes()).unwrap()
    }
    fn publish(&self, path: &str, body: &str) -> Sha256 {
        let text = support::build_whole_file(body);
        fs::write(self.root.path().join(path), &text).unwrap();
        let parsed = parse(&RepoPath::parse(path).unwrap(), text.as_bytes())
            .unwrap()
            .unwrap();
        let element = parsed.elements.into_iter().next().unwrap();
        let metadata = &element.element;
        let mut storage = self.open_storage();
        let mut unit = storage.begin_cache_write().unwrap();
        unit.delete_outgoing(&metadata.id).unwrap();
        unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
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
        unit.insert_edge(
            &Edge::new(
                metadata.id.clone(),
                EdgeTarget::Knowledge(KnowledgeId::parse("dangling.md").unwrap()),
                EdgeType::parse("supports").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        unit.upsert_file(&FileRow {
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
        unit.commit().unwrap();
        element.search_hash
    }
    fn read_failure(&self, config: &EmbeddingConfig, hash: &Sha256) -> Option<EmbeddingFailure> {
        unwrap_current(
            self.open_storage()
                .begin_read()
                .unwrap()
                .read_failure(&config.get_identity(), hash)
                .unwrap(),
        )
    }
    fn read_vector(&self, config: &EmbeddingConfig, hash: &Sha256) -> Option<Vec<f32>> {
        unwrap_current(
            self.open_storage()
                .begin_read()
                .unwrap()
                .read_vector(&config.get_identity(), hash)
                .unwrap(),
        )
    }
    fn read_query_error(&self) -> Option<QueryError> {
        self.open_storage()
            .begin_read()
            .unwrap()
            .read_query_error()
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
                    .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
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
        let mut storage = self.open_storage();
        let mut read = storage.begin_read().unwrap();
        let matches = read.match_text("knowledge").unwrap();
        Corpus {
            rows,
            files,
            matches: matches.into_iter().map(|item| item.id).collect(),
            revision: read.get_revision(),
        }
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
fn missing_and_invalid_configuration_preserve_corpus_and_prior_vectors() {
    // Arrange
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    server.take_requests();
    // Act and assert
    for bytes in [None, Some(b"invalid".as_slice())] {
        let path = fixture.root.path().join(".letopis/config.json");
        match bytes {
            None => fs::remove_file(path).unwrap(),
            Some(bytes) => fs::write(path, bytes).unwrap(),
        }
        let before = fixture.capture_corpus();
        assert!(matches!(
            service.read_configuration().unwrap(),
            ConfigState::Missing | ConfigState::Invalid(_)
        ));
        assert_eq!(service.drain().unwrap(), DrainOutcome::Unavailable);
        assert_eq!(
            service.embed_query("query").unwrap(),
            QueryEmbedding::Unavailable
        );
        assert_eq!(fixture.read_vector(&config, &hash), Some(vec![1., 2., 3.]));
        assert_eq!(fixture.capture_corpus(), before);
        assert_eq!(server.take_requests().len(), 0);
    }
}

#[test]
fn unreadable_configuration_returns_safe_invalid_state() {
    let fixture = Fixture::new();
    let mut service = fixture.open_service();
    fs::create_dir(fixture.root.path().join(".letopis/config.json")).unwrap();
    let before = fixture.capture_corpus();
    assert!(matches!(
        service.read_configuration().unwrap(),
        ConfigState::Invalid(_)
    ));
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn changed_configuration_resets_vectors_but_equal_identity_keeps_them() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    service.drain().unwrap();
    let before = fixture.capture_corpus();
    assert!(matches!(
        service.read_configuration().unwrap(),
        ConfigState::Ready(_)
    ));
    assert!(fixture.read_vector(&config, &hash).is_some());
    let next = fixture.write_config(&server.get_base_url(), 1);
    service.read_configuration().unwrap();
    assert_eq!(fixture.read_vector(&next, &hash), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn stale_first_same_hash_does_not_starve_valid_second_representative() {
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
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(server.take_requests().len(), 1);
    assert!(fixture.read_vector(&config, &hash).is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn stale_and_missing_elements_skip_without_reservations_or_requests() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let other = fixture.publish("b.md", "other");
    fs::write(fixture.root.path().join("a.md"), "unmarked").unwrap();
    fs::remove_file(fixture.root.path().join("b.md")).unwrap();
    let before = fixture.capture_corpus();
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.read_failure(&config, &hash), None);
    assert_eq!(fixture.read_failure(&config, &other), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn retries_are_additional_and_exhaustion_survives_restart() {
    for (retries, attempts) in [(0, 1), (3, 4)] {
        let fixture = Fixture::new();
        let server = start_failure();
        let config = fixture.write_config(&server.get_base_url(), retries);
        let hash = fixture.publish("a.md", "knowledge");
        let before = fixture.capture_corpus();
        assert_eq!(
            fixture.open_service().drain().unwrap(),
            DrainOutcome::Complete
        );
        assert_eq!(server.take_requests().len(), attempts);
        let failed = fixture.read_failure(&config, &hash).unwrap();
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
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fs::write(fixture.root.path().join("a.md"), [0xff]).unwrap();
    let before = fixture.capture_corpus();
    fixture.open_service().drain().unwrap();
    assert!(fixture.read_failure(&config, &hash).unwrap().attempts >= 1);
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn document_read_failure_uses_the_same_durable_budget() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fs::remove_file(fixture.root.path().join("a.md")).unwrap();
    fs::create_dir(fixture.root.path().join("a.md")).unwrap();
    let before = fixture.capture_corpus();
    fixture.open_service().drain().unwrap();
    assert!(fixture.read_failure(&config, &hash).unwrap().attempts >= 1);
    assert_eq!(server.take_requests().len(), 0);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn configured_dimensions_reject_wrong_size_without_graph_changes() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_raw_config(
        &serde_json::json!({"provider":"ollama", "model":"fixture",
        "parameters":{"base_url":server.get_base_url(), "dimensions":4}, "max_retries":0})
        .to_string(),
    );
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    fixture.open_service().drain().unwrap();
    assert_eq!(fixture.read_vector(&config, &hash), None);
    assert_eq!(fixture.read_failure(&config, &hash).unwrap().attempts, 1);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn query_failure_is_single_attempt_without_document_reservation_or_vector() {
    let fixture = Fixture::new();
    let server = start_failure();
    let config = fixture.write_config(&server.get_base_url(), 3);
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    assert_eq!(
        fixture.open_service().embed_query("query").unwrap(),
        QueryEmbedding::Unavailable
    );
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(fixture.read_failure(&config, &hash), None);
    assert_eq!(fixture.read_vector(&config, &hash), None);
    assert!(fixture.read_query_error().is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn query_success_clears_error_without_caching_or_document_attempts() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut storage = fixture.open_storage();
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&config.get_identity()).unwrap());
    unit.set_query_error(1, "old failure").unwrap();
    unit.commit().unwrap();
    let before = fixture.capture_corpus();
    assert_eq!(
        fixture.open_service().embed_query("query").unwrap(),
        QueryEmbedding::Available {
            identity: config.get_identity(),
            vector: vec![1., 2., 3.]
        }
    );
    assert_eq!(fixture.read_query_error(), None);
    assert_eq!(fixture.read_vector(&config, &hash), None);
    assert_eq!(fixture.read_failure(&config, &hash), None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn persistent_lock_prevents_another_drain() {
    let fixture = Fixture::new();
    let server = start_success();
    fixture.write_config(&server.get_base_url(), 0);
    fixture.publish("a.md", "knowledge");
    let files = open_embedding_files(fixture.root.path()).unwrap();
    let guard = files.try_lock_embed().unwrap().unwrap();
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
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let old = fixture.publish("z.md", "old knowledge");
    let mut service = fixture.open_service();
    let worker = Operation::spawn(barrier.clone(), move || service.drain().unwrap());
    barrier.wait_until_entered();
    assert_eq!(fixture.read_failure(&config, &old).unwrap().attempts, 1);
    assert_eq!(fixture.read_vector(&config, &old), None);
    let new = fixture.publish("a.md", "new knowledge");
    let before = fixture.capture_corpus();
    barrier.release();
    assert_eq!(worker.join().unwrap(), DrainOutcome::Complete);
    assert_eq!(server.take_requests().len(), 2);
    assert!(fixture.read_vector(&config, &old).is_some());
    assert!(fixture.read_vector(&config, &new).is_some());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn edit_during_compute_stores_captured_hash_and_keeps_new_hash_pending() {
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let old = fixture.publish("a.md", "old knowledge");
    let mut service = fixture.open_service();
    let worker = Operation::spawn(barrier.clone(), move || service.drain().unwrap());
    barrier.wait_until_entered();
    fs::write(
        fixture.root.path().join("a.md"),
        support::build_whole_file("new knowledge"),
    )
    .unwrap();
    let before = fixture.capture_corpus();
    barrier.release();
    assert_eq!(worker.join().unwrap(), DrainOutcome::Complete);
    assert!(fixture.read_vector(&config, &old).is_some());
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(fixture.capture_corpus(), before);
    let new = fixture.publish("a.md", "new knowledge");
    assert_eq!(fixture.read_vector(&config, &new), None);
    assert!(
        unwrap_current(
            fixture
                .open_storage()
                .begin_read()
                .unwrap()
                .read_next_embedding_work(&config.get_identity(), 1, None)
                .unwrap()
        )
        .is_some()
    );
}

#[test]
fn reset_during_http_discards_old_result_without_old_cleanup() {
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    let worker = Operation::spawn(barrier.clone(), move || service.drain().unwrap());
    barrier.wait_until_entered();
    let next = fixture.write_config(&server.get_base_url(), 1);
    fixture.open_service().read_configuration().unwrap();
    let mut storage = fixture.open_storage();
    let mut unit = unwrap_current(storage.begin_vector_write(&next.get_identity()).unwrap());
    unit.store_vector(&hash, &[1., 2., 3.], 3).unwrap();
    unit.set_query_error(7, "current failure").unwrap();
    unit.commit().unwrap();
    let before = fixture.capture_corpus();
    barrier.release();
    assert_eq!(worker.join().unwrap(), DrainOutcome::ConfigChanged);
    assert!(fixture.read_vector(&next, &hash).is_some());
    assert_eq!(fixture.read_query_error().unwrap().error, "current failure");
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn late_old_query_success_or_error_cannot_replace_new_query_state() {
    for (body, new_success) in [
        (r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#, false),
        ("malformed response", false),
        ("malformed response", true),
    ] {
        let fixture = Fixture::new();
        let (old_server, barrier) = start_paused(body);
        fixture.write_config(&old_server.get_base_url(), 0);
        fixture.publish("a.md", "knowledge");
        let mut old_service = fixture.open_service();
        let worker = Operation::spawn(barrier.clone(), move || {
            old_service.embed_query("query").unwrap()
        });
        barrier.wait_until_entered();
        let new_server = if new_success {
            start_success()
        } else {
            start_failure()
        };
        fixture.write_config(&new_server.get_base_url(), 0);
        let outcome = fixture.open_service().embed_query("new query").unwrap();
        assert_eq!(
            matches!(outcome, QueryEmbedding::Available { .. }),
            new_success
        );
        let error = fixture.read_query_error();
        let before = fixture.capture_corpus();
        barrier.release();
        assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
        assert_eq!(fixture.read_query_error(), error);
        assert_eq!(fixture.capture_corpus(), before);
    }
}

#[test]
fn query_rechecks_dimension_established_during_http() {
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let mut service = fixture.open_service();
    let worker = Operation::spawn(barrier.clone(), move || {
        service.embed_query("query").unwrap()
    });
    barrier.wait_until_entered();
    let mut storage = fixture.open_storage();
    let mut unit = unwrap_current(storage.begin_vector_write(&config.get_identity()).unwrap());
    unit.store_vector(&hash, &[1., 2., 3., 4.], 4).unwrap();
    unit.commit().unwrap();
    let before = fixture.capture_corpus();
    barrier.release();
    assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
    assert!(fixture.read_query_error().is_some());
    assert_eq!(
        fixture.read_vector(&config, &hash),
        Some(vec![1., 2., 3., 4.])
    );
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn cleanup_retains_skipped_and_exhausted_references_and_removes_orphans() {
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 1);
    let skipped = fixture.publish("a.md", "knowledge");
    let exhausted = fixture.publish("b.md", "exhausted");
    let known = fixture.publish("c.md", "known");
    fs::write(fixture.root.path().join("a.md"), "unmarked").unwrap();
    let mut storage = fixture.open_storage();
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    let orphan = Sha256::digest(b"orphan");
    let mut unit = unwrap_current(storage.begin_vector_write(&config.get_identity()).unwrap());
    unit.reserve_attempt(&exhausted, 2).unwrap();
    unit.reserve_attempt(&exhausted, 2).unwrap();
    unit.reserve_attempt(&skipped, 2).unwrap();
    unit.record_failure(&skipped, "prior skipped failure")
        .unwrap();
    unit.reserve_attempt(&orphan, 2).unwrap();
    unit.store_vector(&known, &[1., 2., 3.], 3).unwrap();
    unit.record_failure(&exhausted, "failure").unwrap();
    unit.store_vector(&orphan, &[1., 2., 3.], 3).unwrap();
    unit.commit().unwrap();
    let before = fixture.capture_corpus();
    assert_eq!(
        fixture.open_service().drain().unwrap(),
        DrainOutcome::Complete
    );
    assert_eq!(fixture.read_vector(&config, &orphan), None);
    assert_eq!(
        fixture.read_failure(&config, &exhausted).unwrap().attempts,
        2
    );
    assert_eq!(fixture.read_failure(&config, &skipped).unwrap().attempts, 1);
    assert_eq!(fixture.read_failure(&config, &orphan), None);
    assert!(fixture.read_vector(&config, &known).is_some());
    assert!(
        storage
            .begin_read()
            .unwrap()
            .read_node(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn vector_storage_failure_propagates_internal_error_without_false_provider_failure() {
    // Arrange
    let fixture = Fixture::new();
    let server = start_success();
    let config = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    let before = fixture.capture_corpus();
    let mut service = fixture.open_service();
    let db = Connection::open(fixture.root.path().join("vectors")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_key BEFORE INSERT ON vector_keys
        BEGIN SELECT RAISE(ABORT, 'private storage payload'); END;",
    )
    .unwrap();
    // Act
    let error = service.drain().unwrap_err();
    db.execute_batch("DROP TRIGGER reject_key;").unwrap();
    // Assert
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("private"));
    assert_eq!(fixture.read_vector(&config, &hash), None);
    let failed = fixture.read_failure(&config, &hash).unwrap();
    assert_eq!(failed.attempts, 1);
    assert_eq!(failed.error, None);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn valid_shape_with_invalid_endpoint_does_not_reset_or_compute() {
    let fixture = Fixture::new();
    let server = start_success();
    let prior = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fixture.open_service().drain().unwrap();
    server.take_requests();
    fixture.write_config("ftp://localhost/api", 0);
    let before = fixture.capture_corpus();
    assert!(matches!(
        fixture.open_service().read_configuration().unwrap(),
        ConfigState::Invalid(_)
    ));
    assert!(fixture.read_vector(&prior, &hash).is_some());
    assert!(server.take_requests().is_empty());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn captured_query_checks_identity_before_http_and_before_diagnostic_commit() {
    // Missing identity and replacement before invocation both forbid HTTP.
    for replacement in [false, true] {
        let fixture = Fixture::new();
        let server = start_success();
        let captured = fixture.write_config(&server.get_base_url(), 0);
        if replacement {
            let next = fixture.write_config(&server.get_base_url(), 1);
            seed_query_error(&fixture, &next);
        }
        let error = fixture.read_query_error();
        let result = fixture
            .open_service()
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &captured,
                dimensions: None,
            })
            .unwrap();
        assert_eq!(result, QueryEmbedding::Unavailable);
        assert!(server.take_requests().is_empty());
        assert_eq!(fixture.read_query_error(), error);
    }
}

#[test]
fn captured_query_preserves_replacement_diagnostics_after_http() {
    // Replacement during success or failure must retain its own diagnostic.
    for body in [
        r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#,
        "malformed",
    ] {
        let fixture = Fixture::new();
        let (server, barrier) = start_paused(body);
        let captured = fixture.write_config(&server.get_base_url(), 0);
        fixture.open_service().read_configuration().unwrap();
        let mut service = fixture.open_service();
        let worker = Operation::spawn(barrier.clone(), move || {
            service
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
        let error = fixture.read_query_error();
        barrier.release();
        assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
        assert_eq!(server.take_requests().len(), 1);
        assert_eq!(fixture.read_query_error(), error);
    }
}

#[test]
fn captured_query_validates_pinned_and_newly_established_dimensions() {
    // Pinned size must be checked before a document establishes the index size.
    let fixture = Fixture::new();
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture-alias","embeddings":[[1,2]]}"#.into(),
    });
    let captured = fixture.write_config(&server.get_base_url(), 0);
    fixture.open_service().read_configuration().unwrap();
    let result = fixture
        .open_service()
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "query",
            config: &captured,
            dimensions: NonZeroUsize::new(3),
        })
        .unwrap();
    assert_eq!(result, QueryEmbedding::Unavailable);
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(
        fixture.read_query_error().unwrap().error,
        "invalid embedding response"
    );
}

#[test]
fn captured_query_rejects_dimensions_established_during_http() {
    // First document vector can establish another size while inference is paused.
    let fixture = Fixture::new();
    let (server, barrier) = start_paused(r#"{"model":"fixture-alias","embeddings":[[1,2,3]]}"#);
    let captured = fixture.write_config(&server.get_base_url(), 0);
    let hash = fixture.publish("a.md", "knowledge");
    fixture.open_service().read_configuration().unwrap();
    let config = captured.clone();
    let mut service = fixture.open_service();
    let worker = Operation::spawn(barrier.clone(), move || {
        service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &captured,
                dimensions: None,
            })
            .unwrap()
    });
    barrier.wait_until_entered();
    let mut storage = fixture.open_storage();
    let mut unit = unwrap_current(storage.begin_vector_write(&config.get_identity()).unwrap());
    unit.store_vector(&hash, &[1., 2., 3., 4.], 4).unwrap();
    unit.commit().unwrap();
    barrier.release();
    assert_eq!(worker.join().unwrap(), QueryEmbedding::Unavailable);
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(
        fixture.read_vector(&config, &hash),
        Some(vec![1., 2., 3., 4.])
    );
    assert_eq!(
        fixture.read_query_error().unwrap().error,
        "invalid embedding response"
    );
}

#[test]
fn captured_query_diagnostic_write_failure_remains_an_error() {
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
            "CREATE TRIGGER reject_query_insert BEFORE INSERT ON query_error
             BEGIN SELECT RAISE(ABORT, 'private diagnostic'); END;
             CREATE TRIGGER reject_query_delete BEFORE DELETE ON query_error
             BEGIN SELECT RAISE(ABORT, 'private diagnostic'); END;",
        )
        .unwrap();
        // Act
        let error = service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &config,
                dimensions: None,
            })
            .unwrap_err();
        db.execute_batch("DROP TRIGGER reject_query_insert; DROP TRIGGER reject_query_delete;")
            .unwrap();
        // Assert
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
        assert!(!format!("{error:?}").contains("private"));
        assert_eq!(
            fixture.read_query_error().unwrap().error,
            "replacement diagnostic"
        );
        assert_eq!(server.take_requests().len(), 1);
    }
}

fn seed_query_error(fixture: &Fixture, config: &EmbeddingConfig) {
    let mut storage = fixture.open_storage();
    let observed = storage.begin_read().unwrap().get_config_identity();
    storage
        .reset_vectors(observed.as_ref(), &config.get_identity())
        .unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&config.get_identity()).unwrap());
    unit.set_query_error(7, "replacement diagnostic").unwrap();
    unit.commit().unwrap();
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

fn unwrap_current<T>(value: VectorOutcome<T>) -> T {
    match value {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected configuration change"),
    }
}
