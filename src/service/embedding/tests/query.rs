use super::{Files, Provider, open_storage, publish};
use crate::{
    domain::Sha256,
    markdown::MarkdownParser,
    service::{
        Embedder, EmbeddingConfig, EmbeddingErrorKind, EmbeddingFailure, EmbeddingService,
        QueryEmbedding, QueryEmbeddingRequest, Storage, StorageRead, VectorOutcome, VectorWrite,
    },
    storage::SqliteStorage,
};
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::Ordering},
};
use tempfile::TempDir;

#[test]
fn service_defensively_rejects_provider_cardinality_and_nonfinite_vectors() {
    for vectors in [
        vec![],
        vec![vec![]],
        vec![vec![f32::NAN, 1.]],
        vec![vec![1., 2.]; 2],
    ] {
        // Given: captured configuration and controlled provider output.
        let root = TempDir::new().unwrap();
        let mut provider = Provider::new();
        provider.vectors = vectors;
        let provider = Arc::new(provider);
        let mut service = EmbeddingService::new(
            open_storage(&root),
            Files::new(0),
            MarkdownParser,
            Arc::<Provider>::clone(&provider),
        );
        // When: the query service requests its vector.
        assert_eq!(
            service.embed_query("query").unwrap(),
            QueryEmbedding::Unavailable
        );
        // Then: the vector result and safe stored diagnostic match the scenario.
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
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
    // Given: captured configuration and controlled provider output.
    let root = TempDir::new().unwrap();
    let mut files = Files::new(0);
    files.time = u64::MAX;
    let provider = Provider::new();
    *provider.failure.lock().unwrap() = Some(EmbeddingErrorKind::Connection);
    let provider = Arc::new(provider);
    let mut service = EmbeddingService::new(
        open_storage(&root),
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: the query service requests its vector.
    let error = service.embed_query("query").unwrap_err();
    // Then: the vector result and safe stored diagnostic match the scenario.
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
    let provider = Arc::new(Provider::new());
    let mut service = EmbeddingService::new(
        open_storage(&root),
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: this provider double returns three components despite the request.
    let result = service.embed_query("query").unwrap();
    // Then: a malformed result cannot become an available query vector.
    assert_eq!(result, QueryEmbedding::Unavailable);
    let error = service
        .storage
        .begin_read()
        .unwrap()
        .read_query_error()
        .unwrap()
        .unwrap();
    assert_eq!(error.error, "invalid embedding response");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn captured_query_never_rereads_or_resets_configuration() {
    // Given: the captured identity is committed; the file now contains another model.
    let root = TempDir::new().unwrap();
    let files = Files::new(0);
    let captured = files.get_configured();
    let mut storage = open_storage(&root);
    storage
        .reset_vectors(None, &captured.get_identity())
        .unwrap();
    let provider = Arc::new(Provider::new());
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    service.files.config.replace(Some(br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"new-model","parameters":{"base_url":"http://localhost/api"},"max_retries":0}}"#.to_vec()));
    service.files.reject_config_reads = true;
    // When: the query service requests its vector.
    let result = service
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "captured query",
            config: &captured,
            dimensions: NonZeroUsize::new(3),
        })
        .unwrap();
    // Then: the vector result and safe stored diagnostic match the scenario.
    assert_eq!(
        result,
        QueryEmbedding::Available {
            identity: captured.get_identity(),
            vector: vec![1., 2., 3.]
        }
    );
    assert_eq!(service.files.config_reads.get(), 0);
    assert_eq!(
        *provider.requests.lock().unwrap(),
        vec![(
            captured.get_identity(),
            "captured query".into(),
            NonZeroUsize::new(3)
        )]
    );
    assert_eq!(
        service.storage.begin_read().unwrap().get_config_identity(),
        Some(captured.get_identity())
    );
}

#[test]
fn captured_query_preserves_readiness_policy_and_document_attempts() {
    // Given: a reserved document attempt must survive every query outcome.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    let mut files = Files::new(0);
    let (captured, hash) = reserve_published_document(&mut storage, &mut files);
    files.reject_config_reads = true;
    let provider = Arc::new(Provider::new());
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When / Then: standalone readiness maps unreadable config to unavailable.
    assert_eq!(
        service.embed_query("query").unwrap(),
        QueryEmbedding::Unavailable
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    for failure in [true, false] {
        // Given: captured configuration and controlled provider output.
        *provider.failure.lock().unwrap() = failure.then_some(EmbeddingErrorKind::Credentials);
        // When: the query service requests its vector.
        let result = service
            .embed_query_with_config(QueryEmbeddingRequest {
                text: "query",
                config: &captured,
                dimensions: None,
            })
            .unwrap();
        // Then: the vector result and safe stored diagnostic match the scenario.
        assert_eq!(matches!(result, QueryEmbedding::Unavailable), failure);
        let mut read = service.storage.begin_read().unwrap();
        assert_eq!(
            read.read_query_error().unwrap().map(|error| error.error),
            failure.then(|| "embedding credentials unavailable".into())
        );
        assert_eq!(
            read.read_vector(&captured.get_identity(), &hash).unwrap(),
            VectorOutcome::Current(None)
        );
        assert_eq!(
            read.read_failure(&captured.get_identity(), &hash).unwrap(),
            VectorOutcome::Current(Some(EmbeddingFailure {
                search_hash: hash,
                attempts: 1,
                error: None
            }))
        );
    }
    assert_eq!(service.files.config_reads.get(), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(service.files.waits.borrow().is_empty());
}

#[test]
fn captured_query_rejects_invalid_provider_policy_without_reset_or_http() {
    // Given: captured configuration and controlled provider output.
    let root = TempDir::new().unwrap();
    let files = Files::new(0);
    let captured = files.get_configured();
    let mut storage = open_storage(&root);
    storage
        .reset_vectors(None, &captured.get_identity())
        .unwrap();
    let mut provider = Provider::new();
    provider.invalid_config = true;
    let provider = Arc::new(provider);
    let mut service = EmbeddingService::new(
        storage,
        files,
        MarkdownParser,
        Arc::<Provider>::clone(&provider),
    );
    // When: the query service requests its vector.
    let result = service
        .embed_query_with_config(QueryEmbeddingRequest {
            text: "query",
            config: &captured,
            dimensions: None,
        })
        .unwrap();
    // Then: the vector result and safe stored diagnostic match the scenario.
    assert_eq!(result, QueryEmbedding::Unavailable);
    assert_eq!(service.files.config_reads.get(), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.storage.begin_read().unwrap().get_config_identity(),
        Some(captured.get_identity())
    );
}

fn reserve_published_document(
    storage: &mut SqliteStorage,
    files: &mut Files,
) -> (EmbeddingConfig, Sha256) {
    let hash = publish(storage, files, "a.md", "knowledge");
    let captured = files.get_configured();
    storage
        .reset_vectors(None, &captured.get_identity())
        .unwrap();
    let VectorOutcome::Current(mut unit) = storage
        .begin_vector_write(&captured.get_identity())
        .unwrap()
    else {
        panic!("expected current configuration")
    };
    unit.reserve_attempt(&hash, 1).unwrap();
    unit.commit().unwrap();
    (captured, hash)
}
