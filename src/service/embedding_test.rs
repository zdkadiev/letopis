use crate::service::{
    CapturedFile, Clock, FileError, FileErrorKind, FileIdentity, FileLocker, FileReader,
    FileResult, FileStat,
};

use std::rc::Rc;

use crate::service::StorageError;
use crate::service::TransactionError;
use crate::storage::get_current;

use super::EmbeddingService;
use crate::{
    domain::{ElementKind, KnowledgeId, RepoPath, Sha256},
    markdown::{MarkdownParser, file_parse},
    service::{
        CacheDatabase, ConfigState, Configuration, DrainOutcome, Embedder, EmbeddingConfig,
        EmbeddingError, EmbeddingErrorKind, EmbeddingFailure, EmbeddingProvider, EmbeddingResult,
        KnowledgeMetadata, NodeMetadata, QueryEmbedding, QueryEmbeddingRequest, StorageRead,
        VectorDatabase, VectorOutcome,
    },
    storage::TestDatabases,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::NonZeroUsize,
    time::Duration,
};
use tempfile::TempDir;

#[test]
fn strict_capture_preserves_input_and_os_failure_classifications() {
    for kind in [
        FileErrorKind::NonRegular,
        FileErrorKind::Changed,
        FileErrorKind::UnsafePath,
    ] {
        // Given: an invalid entry or a verified content/identity change without an OS failure.
        let cause = FileError::new(kind);
        // When: the embedding consumer maps the generic capture failure.
        let error = super::map_capture_error(cause);
        // Then: the pre-migration invalid-input classification and safe message remain.
        assert_eq!(error.get_kind(), EmbeddingErrorKind::Input);
        assert_eq!(error.to_string(), "invalid embedding input");
    }
    for kind in [
        FileErrorKind::Io,
        FileErrorKind::Changed,
        FileErrorKind::UnsafePath,
    ] {
        // Given: a permission, deletion or no-follow syscall failure with its private cause.
        let cause = FileError::new_with_cause(kind, std::io::Error::other("PRIVATE_IO"));
        // When: the embedding consumer maps the failed OS operation.
        let error = super::map_capture_error(cause);
        // Then: the infrastructure classification and safe message remain.
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
        assert_eq!(error.to_string(), "embedding operation failed");
    }
}

#[test]
fn query_rejects_inherited_writers_before_inference() {
    // Given: a ready service inside either cache or vector publication.
    for cache_writer in [true, false] {
        let root = TempDir::new().unwrap();
        let mut service = build_embedding_service(
            open_storage(&root),
            Files::new(0),
            MarkdownParser,
            Provider::new(),
        );
        let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
            panic!("ready configuration")
        };
        let cache = std::rc::Rc::clone(&service.cache);
        let vectors = std::rc::Rc::clone(&service.vector_database);
        // When: the parent catches rejection of a captured query request.
        let mut invoke = || {
            let error = service
                .embed_query_with_config(QueryEmbeddingRequest {
                    text: "query",
                    config: &config,
                    dimensions: None,
                })
                .unwrap_err();
            assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
            Ok::<_, StorageError>(())
        };
        let result = if cache_writer {
            cache.with_write_tx(&mut invoke)
        } else {
            vectors
                .with_write_tx(&config.get_identity(), &mut invoke)
                .map(get_current)
        };
        // Then: no provider call occurs and the parent cannot commit after catching the error.
        assert!(matches!(result, Err(TransactionError::Storage(_))));
        assert_eq!(service.provider.calls.get(), 0);
        assert_eq!(
            cache
                .with_read_tx(|| cache.query_error_get_by_id())
                .unwrap(),
            None
        );
    }
}

#[test]
fn query_rejects_an_inherited_vector_read_before_inference() {
    // Given: query publication cannot upgrade an inherited vector read.
    let root = TempDir::new().unwrap();
    let mut service = build_embedding_service(
        open_storage(&root),
        Files::new(0),
        MarkdownParser,
        Provider::new(),
    );
    let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
        panic!("ready configuration")
    };
    let vectors = std::rc::Rc::clone(&service.vector_database);
    // When: the parent catches rejection before inference or diagnostic publication.
    let result = vectors.with_read_tx(&config.get_identity(), || {
        let error = service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &config,
                dimensions: None,
            })
            .unwrap_err();
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
        Ok::<_, StorageError>(())
    });
    // Then: no provider call occurs and catching the rejection cannot rescue the read.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(service.provider.calls.get(), 0);
}

#[test]
fn query_keeps_inference_available_inside_a_pinned_cache_read() {
    // Given: the supported search shape with an independent vector writer.
    let root = TempDir::new().unwrap();
    let mut service = build_embedding_service(
        open_storage(&root),
        Files::new(0),
        MarkdownParser,
        Provider::new(),
    );
    let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
        panic!("ready configuration")
    };
    let cache = std::rc::Rc::clone(&service.cache);
    // When: query inference runs while the cache read remains pinned.
    let result = cache
        .with_read_tx(|| {
            service.embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &config,
                dimensions: None,
            })
        })
        .unwrap();
    // Then: exactly one call returns the independent literal vector.
    assert!(
        matches!(result, QueryEmbedding::Available { vector, .. } if vector == vec![1.0, 2.0, 3.0])
    );
    assert_eq!(service.provider.calls.get(), 1);
}

#[test]
fn drain_rejects_inherited_cache_scopes_before_selecting_work() {
    // Given: one eligible document and a provider that bounds a repeated-selection bug.
    for cache_writer in [false, true] {
        let root = TempDir::new().unwrap();
        let mut databases = open_storage(&root);
        let mut files = Files::new(0);
        let hash = publish(&mut databases, &mut files, "a.md", "eligible text");
        let mut provider = Provider::new();
        provider.max_calls = Some(1);
        let mut service = build_embedding_service(databases, files, MarkdownParser, provider);
        let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
            panic!("ready configuration")
        };
        let cache = std::rc::Rc::clone(&service.cache);
        // When: a parent catches drain's rejection within an inherited snapshot or writer.
        let mut invoke = || {
            let error = service.drain().unwrap_err();
            assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
            Ok::<_, StorageError>(())
        };
        let result = if cache_writer {
            cache.with_write_tx(&mut invoke)
        } else {
            cache.with_read_tx(&mut invoke)
        };
        // Then: no work is reserved or inferred, and the rejected parent is rollback-only.
        assert!(matches!(result, Err(TransactionError::Storage(_))));
        assert_eq!(service.provider.calls.get(), 0);
        assert_eq!(
            cache
                .with_read_tx(
                    || cache.embed_failures_get_by_search_hash(&config.get_identity(), &hash)
                )
                .unwrap(),
            VectorOutcome::Current(None)
        );
        // When: the same service drains outside any inherited scope.
        assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
        // Then: fresh work selection sees publication and terminates after exactly one call.
        assert_eq!(service.provider.calls.get(), 1);
    }
}

#[test]
fn reservation_joins_the_shared_vector_scope_and_rolls_back_with_its_parent() {
    use crate::service::VectorDatabase;
    use rusqlite::Connection;
    // Given: a ready embedding service and an independent vector observer.
    let root = TempDir::new().unwrap();
    let mut service = crate::service::embedding::tests::build_embedding_service(
        open_storage(&root),
        Files::new(0),
        MarkdownParser,
        Provider::new(),
    );
    let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
        panic!("configuration must be ready")
    };
    let vectors = std::rc::Rc::clone(&service.vector_database);
    let hash = Sha256::digest(b"joined attempt");
    let observer = Connection::open(root.path().join("vectors")).unwrap();
    // When: a nested reservation succeeds but its parent rejects the operation.
    let result = vectors.with_write_tx(&config.get_identity(), || -> EmbeddingResult<()> {
        assert!(matches!(
            service.reserve(&config, &hash)?,
            VectorOutcome::Current(true)
        ));
        assert_eq!(
            observer
                .query_row("select count(*) from embed_failures", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        Err(EmbeddingError::new(EmbeddingErrorKind::Input))
    });
    // Then: the attempt is rolled back, and a later standalone reservation commits once.
    assert!(matches!(
        result,
        Err(TransactionError::Operation(error)) if error.get_kind() == EmbeddingErrorKind::Input
    ));
    assert_eq!(
        observer
            .query_row("select count(*) from embed_failures", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(matches!(
        service.reserve(&config, &hash).unwrap(),
        VectorOutcome::Current(true)
    ));
    assert_eq!(
        observer
            .query_row("select attempts from embed_failures", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

mod tasks {
    use super::{Files, Provider, open_storage, publish};
    use crate::service::TransactionError;
    use crate::{
        markdown::MarkdownParser,
        service::{CacheDatabase, CommandErrorKind, EmbeddingExecutor},
    };
    use rusqlite::Connection;
    use tempfile::TempDir;

    #[test]
    fn readiness_uses_durable_work_without_http_lock_or_retained_read() {
        // Given: durable pending embedding work.
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        let mut files = Files::new(0);
        publish(&mut storage, &mut files, "a.md", "ready token");
        let mut service = crate::service::embedding::tests::build_embedding_service(
            storage,
            files,
            MarkdownParser,
            Provider::new(),
        );
        // When: readiness queries the scoped storage view.
        assert!(service.is_work_available().unwrap());
        // Then: no document/provider work; the read is gone before another unit starts.
        assert_eq!(service.provider.calls.get(), 0);
        assert!(service.file_reader.reads.borrow().is_empty());
        assert!(matches!(
            service.cache.with_write_tx(|| Err::<(), _>(())),
            Err(TransactionError::Operation(()))
        ));
    }

    #[test]
    fn readiness_preserves_storage_failure_as_a_safe_typed_error() {
        // Given
        let root = TempDir::new().unwrap();
        let mut service = crate::service::embedding::tests::build_embedding_service(
            open_storage(&root),
            Files::new(0),
            MarkdownParser,
            Provider::new(),
        );
        assert!(!service.is_work_available().unwrap());
        Connection::open(root.path().join("cache"))
            .unwrap()
            .execute_batch("alter table nodes rename to private_missing_nodes")
            .unwrap();
        // When: durable-read failure must not look like successful no-work.
        let error = service.is_work_available().unwrap_err();
        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "embedding operation failed");
        assert!(!format!("{error:?}").contains("private_missing_nodes"));
    }
}

struct Files {
    config: RefCell<Option<Vec<u8>>>,
    documents: BTreeMap<String, Vec<u8>>,
    reads: RefCell<BTreeMap<String, usize>>,
    waits: Rc<RefCell<Vec<Duration>>>,
    time: u64,
    config_reads: Cell<usize>,
    is_rejecting_config_reads: bool,
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
            waits: Rc::default(),
            time: 17,
            config_reads: Cell::new(0),
            is_rejecting_config_reads: false,
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

impl FileReader for Files {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        panic!("unexpected file_get_by_path")
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        let bytes = if path == std::path::Path::new(".letopis/config.json") {
            self.config_reads.set(self.config_reads.get() + 1);
            if self.is_rejecting_config_reads {
                return Err(FileError::new(FileErrorKind::Io));
            }
            self.config.borrow().clone()
        } else {
            *self
                .reads
                .borrow_mut()
                .entry(path.to_str().unwrap().into())
                .or_default() += 1;
            self.documents.get(path.to_str().unwrap()).cloned()
        };
        Ok(bytes)
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        panic!("unused stat")
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unused identity")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unused existence")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unused verification")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unused distinctness")
    }

    fn directory_get_by_path(
        &self,
        path: &std::path::Path,
    ) -> FileResult<Option<std::os::fd::OwnedFd>> {
        assert_eq!(path, std::path::Path::new(".letopis"));
        Ok(None)
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        panic!("unused lock observation")
    }
}

impl FileLocker for Files {
    type Lock = ();

    fn lock_acquire_by_path(&self, _: &std::path::Path) -> FileResult<()> {
        panic!("unused blocking lock")
    }

    fn lock_try_acquire_by_path(&self, path: &std::path::Path) -> FileResult<Option<()>> {
        assert_eq!(path, std::path::Path::new(".letopis/embed.lock"));
        Ok(Some(()))
    }
}

#[derive(Debug)]
struct TestClock {
    waits: Rc<RefCell<Vec<Duration>>>,
    time: u64,
}

impl Clock for TestClock {
    fn clock_get_time_ns(&self) -> std::io::Result<i64> {
        panic!("unused nanoseconds")
    }

    fn clock_get_time_secs(&self) -> std::io::Result<u64> {
        Ok(self.time)
    }

    fn clock_wait(&self, delay: Duration) -> std::io::Result<()> {
        self.waits.borrow_mut().push(delay);
        Ok(())
    }
}

struct Provider {
    calls: Cell<usize>,
    max_calls: Option<usize>,
    is_config_invalid: bool,
    failure: Option<EmbeddingErrorKind>,
    vectors: Vec<Vec<f32>>,
    requests: RefCell<Vec<(Sha256, String, Option<NonZeroUsize>)>>,
}

impl Provider {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            max_calls: None,
            is_config_invalid: false,
            failure: None,
            vectors: vec![vec![1., 2., 3.]],
            requests: RefCell::default(),
        }
    }
}

impl EmbeddingProvider for Provider {
    fn config_validate(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
        if self.is_config_invalid {
            Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
        } else {
            Ok(())
        }
    }

    fn vectors_compute_by_texts(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.calls.set(self.calls.get() + 1);
        assert!(
            self.max_calls.is_none_or(|limit| self.calls.get() <= limit),
            "provider call bound exceeded"
        );
        self.requests
            .borrow_mut()
            .push((config.get_identity(), texts[0].into(), dimensions));
        match self.failure {
            Some(kind) => Err(EmbeddingError::new(kind)),
            None => Ok(self.vectors.clone()),
        }
    }
}

#[test]
fn configuration_is_reread_and_provider_validation_precedes_identity_reset() {
    // Given: configuration changes and a provider requiring validation.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root);
    let old = Sha256::digest(b"old identity");
    storage.vector_storage.reset_vectors(None, &old).unwrap();
    let mut provider = Provider::new();
    provider.is_config_invalid = true;
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        Files::new(0),
        MarkdownParser,
        provider,
    );
    // When / Then: configuration is retrieved again; provider validation precedes resetting the vector identity.
    assert!(matches!(
        service.get_configuration().unwrap(),
        ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig)
    ));
    assert_eq!(
        service
            .cache
            .with_read_tx(|| service.cache.meta_get_by_id())
            .unwrap(),
        Some(old)
    );
    *service.file_reader.config.borrow_mut() = None;
    assert!(matches!(
        service.get_configuration().unwrap(),
        ConfigState::Missing
    ));
    assert_eq!(service.provider.calls.get(), 0);
}

#[test]
fn skipped_ids_are_never_reread_when_processed_work_restarts_selection() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    publish(&mut storage, &mut files, "a.md", "shared");
    publish(&mut storage, &mut files, "b.md", "shared");
    publish(&mut storage, &mut files, "c.md", "different");
    files.documents.insert("a.md".into(), b"unmarked".to_vec());
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        files,
        MarkdownParser,
        Provider::new(),
    );
    // When
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then
    assert_eq!(service.provider.calls.get(), 2);
    assert_eq!(
        *service.file_reader.reads.borrow(),
        BTreeMap::from([("a.md".into(), 1), ("b.md".into(), 1), ("c.md".into(), 1)])
    );
}

#[test]
fn credential_failures_use_four_durable_attempts_and_three_one_second_waits() {
    // Given: credential failures under a four-attempt budget.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(3);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let mut provider = Provider::new();
    provider.failure = Some(EmbeddingErrorKind::Credentials);
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        files,
        MarkdownParser,
        provider,
    );
    // When: the embedding drain runs to completion.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: four attempts and three retry waits are durable.
    assert_eq!(service.provider.calls.get(), 4);
    assert_eq!(
        *service.file_reader.waits.borrow(),
        vec![Duration::from_secs(1); 3]
    );
    let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
        panic!()
    };
    let VectorOutcome::Current(Some(failure)) = service
        .cache
        .with_read_tx(|| {
            service
                .cache
                .embed_failures_get_by_search_hash(&config.get_identity(), &hash)
        })
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
        // Given: a provider returning invalid cardinality or nonfinite components.
        let root = TempDir::new().unwrap();
        let mut provider = Provider::new();
        provider.vectors = vectors;
        let mut service = crate::service::embedding::tests::build_embedding_service(
            open_storage(&root),
            Files::new(0),
            MarkdownParser,
            provider,
        );
        // When: a query embedding is requested.
        assert_eq!(
            service.embed_query("query").unwrap(),
            QueryEmbedding::Unavailable
        );
        // Then: the invalid provider result is rejected.
        assert_eq!(service.provider.calls.get(), 1);
        assert_eq!(
            service
                .cache
                .with_read_tx(|| service.cache.query_error_get_by_id())
                .unwrap()
                .unwrap()
                .error,
            "invalid embedding response"
        );
    }
}

#[test]
fn unsupported_query_timestamp_is_an_internal_error_without_truncation() {
    // Given: an unsupported query-error timestamp.
    let root = TempDir::new().unwrap();
    let mut files = Files::new(0);
    files.time = u64::MAX;
    let mut provider = Provider::new();
    provider.failure = Some(EmbeddingErrorKind::Connection);
    let mut service = crate::service::embedding::tests::build_embedding_service(
        open_storage(&root),
        files,
        MarkdownParser,
        provider,
    );
    // When: a query embedding fails.
    let error = service.embed_query("query").unwrap_err();
    // Then: the timestamp is rejected without truncation.
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
    assert_eq!(
        service
            .cache
            .with_read_tx(|| service.cache.query_error_get_by_id())
            .unwrap(),
        None
    );
}

#[test]
fn rejected_vector_input_drops_poisoned_unit_before_recording_failure() {
    // Given: a deliberately faulty provider bypasses its native size contract.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
    let documents = files.documents.clone();
    let revision = storage
        .cache_storage
        .with_read_tx(|| storage.cache_storage.project_get_by_id())
        .unwrap();
    let mut provider = Provider::new();
    provider.vectors = vec![vec![1.]];
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        files,
        MarkdownParser,
        provider,
    );
    // When: the drain tries to store the provider result.
    assert_eq!(service.drain().unwrap(), DrainOutcome::Complete);
    // Then: storage rejection rolled back, then a fresh unit persisted the error.
    let ConfigState::Ready(config) = service.get_configuration().unwrap() else {
        panic!()
    };
    let cache = std::rc::Rc::clone(&service.cache);
    cache
        .with_read_tx(|| -> crate::service::StorageResult<()> {
            let read = cache.as_ref();
            let VectorOutcome::Current(Some(failure)) = read
                .embed_failures_get_by_search_hash(&config.get_identity(), &hash)
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(failure.attempts, 1);
            assert_eq!(failure.error.unwrap(), "invalid embedding response");
            assert_eq!(
                read.vector_keys_get_by_search_hash(&config.get_identity(), &hash)
                    .unwrap(),
                VectorOutcome::Current(None)
            );
            assert_eq!(read.project_get_by_id().unwrap(), revision);
            assert_eq!(service.file_reader.documents, documents);
            assert_eq!(
                read.text_index_get_by_expression("knowledge", NonZeroUsize::new(100).unwrap())
                    .unwrap()
                    .len(),
                1
            );

            Ok(())
        })
        .unwrap();
}

mod process {
    use crate::service::StorageError;

    use crate::storage::get_current;

    use super::{Files, open_storage, publish};
    use crate::{
        domain::{RepoPath, Sha256},
        markdown::file_parse,
        service::{
            CacheDatabase, ConfigState, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingExecutor,
            QueryEmbedding, StorageRead, VectorDatabase, VectorOutcome,
        },
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
        let storage =
            crate::storage::open_test_storage(&root.join("cache"), &root.join("vectors")).unwrap();
        let mut service =
            crate::service::embedding::tests::wire_test_embedder(&root, storage).unwrap();
        // When / Then: each action is checked in its assertion.
        if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("query") {
            assert_eq!(
                service.embed_query("canary query").unwrap(),
                QueryEmbedding::Unavailable
            );
        } else if env::var("LETOPIS_PROCESS_ACTION").as_deref() == Ok("readiness") {
            assert!(!service.is_work_available().unwrap());
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
            let storage = open_storage(&self.root);
            storage
                .cache_storage
                .with_read_tx(|| {
                    let read = &storage.cache_storage;
                    assert_eq!(
                        read.vector_keys_get_by_search_hash(&config.get_identity(), &self.hash)
                            .unwrap(),
                        VectorOutcome::Current(vector)
                    );
                    let VectorOutcome::Current(failure) = read
                        .embed_failures_get_by_search_hash(&config.get_identity(), &self.hash)
                        .unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(failure.map(|failure| failure.attempts), attempts);

                    Ok::<_, StorageError>(())
                })
                .unwrap();
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
        fn spawn(
            fixture: &Fixture,
            label: &str,
            stage: &str,
            action: &str,
            expected: &str,
        ) -> Self {
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
        let storage = open_storage(&fixture.root);
        storage
            .cache_storage
            .with_read_tx(|| {
                let read = &storage.cache_storage;
                let bytes = fs::read(fixture.root.path().join("b.md")).unwrap();
                let parsed = file_parse(&RepoPath::parse("b.md").unwrap(), &bytes)
                    .unwrap()
                    .unwrap();
                let hash = parsed.elements[0].search_hash;
                assert_eq!(
                    read.vector_keys_get_by_search_hash(&config.get_identity(), &hash)
                        .unwrap(),
                    VectorOutcome::Current(Some(vec![4., 5., 6.]))
                );

                Ok::<_, StorageError>(())
            })
            .unwrap();
    }

    pub(super) fn reach_checkpoint(stage: &str) {
        if env::var("LETOPIS_PROCESS_STAGE").as_deref() != Ok(stage) {
            return;
        }
        let mut socket =
            UnixStream::connect(env::var_os("LETOPIS_PROCESS_SOCKET").unwrap()).unwrap();
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
        let mut service = crate::service::embedding::tests::wire_test_embedder(
            fixture.root.path(),
            open_storage(&fixture.root),
        )
        .unwrap();
        service.get_configuration().unwrap();
        let mut storage = open_storage(&fixture.root);
        let mut files = Files::new(0);
        let current = publish(&mut storage, &mut files, "b.md", "current knowledge");
        fs::write(fixture.root.path().join("b.md"), &files.documents["b.md"]).unwrap();
        storage
            .vector_storage
            .with_write_tx(&next.get_identity(), || {
                let unit = &storage.vector_storage;
                unit.store_vector(&current, &[4., 5., 6.], 3).unwrap();
                unit.query_error_upsert_by_id(7, "current safe error")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
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
        let storage = open_storage(&fixture.root);
        let error = storage
            .cache_storage
            .with_read_tx(|| Ok::<_, StorageError>(storage.cache_storage.query_error_get_by_id()))
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(error.at, 7);
        assert_eq!(error.error, "current safe error");
    }

    mod configuration {
        use crate::service::StorageError;

        use crate::storage::get_current;

        use super::{
            Fixture, LIMIT, Process, SUCCESS, assert_current_vector, get_http_request,
            open_storage, reply, reset_with_current_state,
        };
        use crate::service::{Embedder, VectorDatabase};
        use std::{
            io::Write,
            thread,
            time::{Duration, Instant},
        };

        #[test]
        fn stale_configuration_observation_preserves_current_vectors_and_attempts() {
            // Given
            for action in ["drain", "query", "readiness", "configuration"] {
                // When / Then: the helper runs and checks the process scenario.
                assert_stale_observation(action);
            }
        }

        #[test]
        fn configuration_change_after_storage_observation_never_resets_newer_state() {
            // Given: P has observed the old stored identity and pauses before reading config.
            let fixture = Fixture::new(0);
            let mut child = Process::spawn(
                &fixture,
                "observed-config",
                "observed",
                "observed",
                "ConfigChanged",
            );
            let mut barrier = child.await_checkpoint();

            // When: B replaces the config and publishes newer vectors and attempts.
            let next = reset_with_current_state(&fixture);
            let storage = open_storage(&fixture.root);
            storage
                .vector_storage
                .with_write_tx(&next.get_identity(), || {
                    let unit = &storage.vector_storage;
                    assert!(
                        unit.embed_failures_upsert_by_search_hash_and_attempts(&fixture.hash, 4)
                            .unwrap()
                    );

                    Ok::<_, StorageError>(())
                })
                .map(get_current)
                .unwrap();
            let before = fixture.capture_cache();
            barrier.write_all(b"C").unwrap();
            let requests = finish_http_requests(&fixture, &mut child);

            // Then: P reads the new config but its old observation prevents a destructive reset.
            fixture.assert_state(&next, None, Some(1));
            assert_current_vector(&fixture, &next);
            assert_eq!(fixture.capture_cache(), before);
            assert_eq!(requests, 0, "stale observation must never reach HTTP");
            child.finish();
        }

        fn assert_stale_observation(action: &str) {
            // Given: P validates A while its original storage observation is still current.
            let fixture = Fixture::new(0);
            let mut service = crate::service::embedding::tests::wire_test_embedder(
                fixture.root.path(),
                open_storage(&fixture.root),
            )
            .unwrap();
            service.get_configuration().unwrap();
            drop(service);
            let mut child = Process::spawn(
                &fixture,
                "stale-config",
                "configuration",
                action,
                "ConfigChanged",
            );
            let mut barrier = child.await_checkpoint();
            let next = reset_with_current_state(&fixture);
            let storage = open_storage(&fixture.root);
            storage
                .vector_storage
                .with_write_tx(&next.get_identity(), || {
                    let unit = &storage.vector_storage;
                    assert!(
                        unit.embed_failures_upsert_by_search_hash_and_attempts(&fixture.hash, 4)
                            .unwrap()
                    );

                    Ok::<_, StorageError>(())
                })
                .map(get_current)
                .unwrap();
            let before = fixture.capture_cache();
            // When: obsolete P resumes after B has stored both successful and pending work.
            barrier.write_all(b"C").unwrap();
            let requests = finish_http_requests(&fixture, &mut child);
            // Then: check stored state first; baseline must fail on destroyed B state.
            fixture.assert_state(&next, None, Some(1));
            assert_current_vector(&fixture, &next);
            assert_eq!(fixture.capture_cache(), before);
            assert_eq!(requests, 0, "obsolete configuration must never reach HTTP");
            child.finish();
        }

        fn finish_http_requests(fixture: &Fixture, child: &mut Process) -> usize {
            let mut requests = 0;
            let start = Instant::now();
            while child.child.try_wait().unwrap().is_none() {
                assert!(start.elapsed() < LIMIT, "child watchdog timeout");
                if let Ok((stream, _)) = fixture.listener.accept() {
                    requests += 1;
                    reply(get_http_request(stream), SUCCESS);
                }
                thread::sleep(Duration::from_millis(5));
            }
            while let Ok((stream, _)) = fixture.listener.accept() {
                requests += 1;
                reply(get_http_request(stream), SUCCESS);
            }
            requests
        }
    }
}

#[test]
fn rejects_a_provider_vector_that_violates_the_first_requested_dimension() {
    // Given: no stored dimension can mask the configuration's requested size.
    let root = TempDir::new().unwrap();
    let files = Files::new(0);
    let config = serde_json::json!({"embedding_model": {
        "enabled":true, "provider":"ollama", "model":"m", "max_retries":0,
        "parameters":{"base_url":"http://localhost/api", "dimensions":2},
    }});
    files
        .config
        .replace(Some(serde_json::to_vec(&config).unwrap()));
    let mut service = crate::service::embedding::tests::build_embedding_service(
        open_storage(&root),
        files,
        MarkdownParser,
        Provider::new(),
    );
    // When: this provider double returns three components despite the request.
    let result = service.embed_query("query").unwrap();
    // Then: a malformed result cannot become an available query vector.
    assert_eq!(result, QueryEmbedding::Unavailable);
    let error = service
        .cache
        .with_read_tx(|| service.cache.query_error_get_by_id())
        .unwrap()
        .unwrap();
    assert_eq!(error.error, "invalid embedding response");
    assert_eq!(service.provider.calls.get(), 1);
}

#[test]
fn captured_query_never_rereads_or_resets_configuration() {
    // Given: the captured identity is committed; the file now contains another model.
    let root = TempDir::new().unwrap();
    let files = Files::new(0);
    let captured = files.get_configured();
    let storage = open_storage(&root);
    storage
        .vector_storage
        .reset_vectors(None, &captured.get_identity())
        .unwrap();
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        files,
        MarkdownParser,
        Provider::new(),
    );
    service.file_reader.config.replace(Some(br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"new-model","parameters":{"base_url":"http://localhost/api"},"max_retries":0}}"#.to_vec()));
    service.file_reader.is_rejecting_config_reads = true;
    // When: the query uses its captured configuration.
    let result = service
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "captured query",
            config: &captured,
            dimensions: NonZeroUsize::new(3),
        })
        .unwrap();
    // Then: configuration is neither reread nor reset.
    assert_eq!(
        result,
        QueryEmbedding::Available {
            identity: captured.get_identity(),
            vector: vec![1., 2., 3.]
        }
    );
    assert_eq!(service.file_reader.config_reads.get(), 0);
    assert_eq!(
        *service.provider.requests.borrow(),
        vec![(
            captured.get_identity(),
            "captured query".into(),
            NonZeroUsize::new(3)
        )]
    );
    assert_eq!(
        service
            .cache
            .with_read_tx(|| service.cache.meta_get_by_id())
            .unwrap(),
        Some(captured.get_identity())
    );
}

#[test]
fn captured_query_preserves_readiness_policy_and_document_attempts() {
    // Given: a reserved document attempt must survive every query outcome.
    let mut fixture = ReservedQueryFixture::new();
    let service = &mut fixture.service;
    let hash = fixture.hash;
    let captured = &fixture.captured;
    // When / Then: standalone readiness maps unreadable config to unavailable.
    assert_eq!(
        service.embed_query("query").unwrap(),
        QueryEmbedding::Unavailable
    );
    assert_eq!(service.provider.calls.get(), 0);
    for failure in [true, false] {
        // Given: captured readiness policy and document attempts.
        service.provider.failure = failure.then_some(EmbeddingErrorKind::Credentials);
        // When: the captured query is embedded.
        let result = service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: captured,
                dimensions: None,
            })
            .unwrap();
        // Then: document attempts and readiness policy remain unchanged.
        assert_eq!(matches!(result, QueryEmbedding::Unavailable), failure);
        let cache = std::rc::Rc::clone(&service.cache);
        cache
            .with_read_tx(|| -> crate::service::StorageResult<()> {
                let read = cache.as_ref();
                assert_eq!(
                    read.query_error_get_by_id()
                        .unwrap()
                        .map(|error| error.error),
                    failure.then(|| "embedding credentials unavailable".into())
                );
                assert_eq!(
                    read.vector_keys_get_by_search_hash(&captured.get_identity(), &hash)
                        .unwrap(),
                    VectorOutcome::Current(None)
                );
                assert_eq!(
                    read.embed_failures_get_by_search_hash(&captured.get_identity(), &hash)
                        .unwrap(),
                    VectorOutcome::Current(Some(EmbeddingFailure {
                        search_hash: hash,
                        attempts: 1,
                        error: None
                    }))
                );

                Ok(())
            })
            .unwrap();
    }
    assert_eq!(service.file_reader.config_reads.get(), 1);
    assert_eq!(service.provider.calls.get(), 2);
    assert!(service.file_reader.waits.borrow().is_empty());
}

#[test]
fn captured_query_rejects_invalid_provider_policy_without_reset_or_http() {
    // Given: an invalid provider policy with existing vector state.
    let root = TempDir::new().unwrap();
    let files = Files::new(0);
    let captured = files.get_configured();
    let storage = open_storage(&root);
    storage
        .vector_storage
        .reset_vectors(None, &captured.get_identity())
        .unwrap();
    let mut provider = Provider::new();
    provider.is_config_invalid = true;
    let mut service = crate::service::embedding::tests::build_embedding_service(
        storage,
        files,
        MarkdownParser,
        provider,
    );
    // When: a captured query is attempted.
    let result = service
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "query",
            config: &captured,
            dimensions: None,
        })
        .unwrap();
    // Then: no provider request or identity reset occurs.
    assert_eq!(result, QueryEmbedding::Unavailable);
    assert_eq!(service.file_reader.config_reads.get(), 0);
    assert_eq!(service.provider.calls.get(), 0);
    assert_eq!(
        service
            .cache
            .with_read_tx(|| service.cache.meta_get_by_id())
            .unwrap(),
        Some(captured.get_identity())
    );
}

fn open_storage(root: &TempDir) -> TestDatabases {
    crate::storage::open_test_storage(&root.path().join("cache"), &root.path().join("vectors"))
        .unwrap()
}

fn publish(storage: &mut TestDatabases, files: &mut Files, path: &str, body: &str) -> Sha256 {
    let bytes = format!("<!-- letopis: {{\"created_at\":1}} -->\n{body}").into_bytes();
    let mut parsed = file_parse(&RepoPath::parse(path).unwrap(), &bytes)
        .unwrap()
        .unwrap();
    let element = parsed.elements.remove(0);
    storage
        .cache_storage
        .with_write_tx(|| {
            let unit = &storage.cache_storage;
            unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
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

            unit.project_get_revision()
        })
        .unwrap();
    files.documents.insert(path.into(), bytes);
    element.search_hash
}

pub(super) fn reach_checkpoint(stage: &str) {
    process::reach_checkpoint(stage);
}

fn build_embedding_service<M: crate::service::MarkdownCodec, P: EmbeddingProvider>(
    storage: TestDatabases,
    files: Files,
    markdown: M,
    provider: P,
) -> EmbeddingService<crate::storage::CacheStorage, crate::storage::VectorStorage, Files, M, P> {
    let (cache, vectors) = (storage.cache_storage, storage.vector_storage);
    let clock = Box::new(TestClock {
        waits: Rc::clone(&files.waits),
        time: files.time,
    });
    EmbeddingService::new(
        std::rc::Rc::new(cache),
        std::rc::Rc::new(vectors),
        files,
        clock,
        markdown,
        provider,
    )
}

fn wire_test_embedder(
    root: &std::path::Path,
    storage: TestDatabases,
) -> EmbeddingResult<impl Embedder + crate::service::EmbeddingExecutor + use<>> {
    let (cache, vectors) = (storage.cache_storage, storage.vector_storage);
    crate::wire_embedder(root, std::rc::Rc::new(cache), std::rc::Rc::new(vectors))
}

struct ReservedQueryFixture {
    service: EmbeddingService<
        crate::storage::CacheStorage,
        crate::storage::VectorStorage,
        Files,
        MarkdownParser,
        Provider,
    >,
    // Keep database files alive until the service has closed its connections.
    _root: TempDir,
    hash: Sha256,
    captured: EmbeddingConfig,
}

impl ReservedQueryFixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        let mut files = Files::new(0);
        let hash = publish(&mut storage, &mut files, "a.md", "knowledge");
        let captured = files.get_configured();
        storage
            .vector_storage
            .reset_vectors(None, &captured.get_identity())
            .unwrap();
        storage
            .vector_storage
            .with_write_tx(&captured.get_identity(), || {
                let unit = &storage.vector_storage;
                unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 1)
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
        files.is_rejecting_config_reads = true;
        let service = crate::service::embedding::tests::build_embedding_service(
            storage,
            files,
            MarkdownParser,
            Provider::new(),
        );
        Self {
            service,
            _root: root,
            hash,
            captured,
        }
    }
}
