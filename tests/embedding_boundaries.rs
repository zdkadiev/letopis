mod support;
use letopis::{
    build_embedding_provider,
    domain::Sha256,
    service::{EmbeddingConfig, EmbeddingErrorKind, EmbeddingProvider},
};
use serde_json::json;
use support::embedding::{FixtureReply, HttpFixture};

#[test]
fn rejects_excessive_object_only_config_nesting() {
    // Given: arrays use a different visitor path and cannot establish this bound.
    let nested = format!("{}0{}", r#"{"child":"#.repeat(130), "}".repeat(130));
    let bytes = format!(
        r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","future":{nested}}},"max_retries":3}}"#
    );
    // When
    let error = EmbeddingConfig::parse(bytes.as_bytes()).unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidConfig);
}

#[test]
fn accepts_valid_responses_between_one_and_ten_mebibytes() {
    // Given: provider extensions are allowed within the response cap.
    let body = json!({"model":"fixture","embeddings":[[1,2,3]],
                      "extra":"x".repeat(2 * 1024 * 1024)});
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: body.to_string(),
    });
    let bytes = serde_json::to_vec(&json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":server.get_base_url()},"max_retries":3}))
    .unwrap();
    let config = EmbeddingConfig::parse(&bytes).unwrap();
    // When: one vector is computed within the ten-mebibyte response limit.
    let vectors = build_embedding_provider()
        .unwrap()
        .vectors_compute_by_texts(&config, &["input"], None)
        .unwrap();
    // Then: the literal vector is returned after exactly one request.
    assert_eq!(vectors, vec![vec![1.0, 2.0, 3.0]]);
    assert_eq!(server.take_requests().len(), 1);
}

#[test]
fn rejects_unsigned_option_values_that_jcs_cannot_preserve() {
    // Given: this unsigned value cannot use the signed-integer fallback.
    let bytes = serde_json::to_vec(&json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://localhost/api","future":u64::MAX},
        "max_retries":3}))
    .unwrap();
    // When
    let error = EmbeddingConfig::parse(&bytes).unwrap_err();
    // Then: canonical identity must not silently round the executed option.
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidConfig);
}

#[test]
fn preserves_valid_nested_array_parameters_in_the_canonical_identity() {
    // Given: unknown parameter values may contain arrays, including empty arrays.
    let future = json!([1, {"key":[2,3]}, [], 4.25]);
    let bytes = serde_json::to_vec(&json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://localhost/api","future":future},
        "max_retries":3}))
    .unwrap();
    let canonical = br#"{"max_retries":3,"model":"fixture","parameters":{"base_url":"http://localhost/api","future":[1,{"key":[2,3]},[],4.25]},"provider":"ollama"}"#;
    // When
    let config = EmbeddingConfig::parse(&bytes).unwrap();
    // Then: execution values and their stored identity preserve the same array.
    assert_eq!(config.get_parameters().get("future"), Some(&future));
    assert_eq!(config.get_identity(), Sha256::digest(canonical));
}
