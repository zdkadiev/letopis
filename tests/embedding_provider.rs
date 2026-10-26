use letopis::build_embedding_provider;
use letopis::service::{EmbeddingConfig, EmbeddingProvider};

#[test]
fn ignores_object_order_for_identity() {
    // Given
    let compact = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost:11434/api"},"max_retries":3}"#;
    let reordered = br#"{ "max_retries": 3, "parameters": {"base_url":"http://localhost:11434/api"}, "model":"fixture", "provider":"ollama" }"#;
    // When
    let first = parse_config(compact);
    let second = parse_config(reordered);
    // Then
    assert_eq!(first.get_identity(), second.get_identity());
    assert_eq!(first.get_max_attempts(), 4);
}

#[test]
fn rejects_duplicate_nested_keys() {
    // Given
    let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","options":{"seed":1,"seed":2}},"max_retries":3}"#;
    // When
    let result = EmbeddingConfig::parse(bytes);
    // Then
    assert!(result.is_err());
}

#[test]
fn validates_endpoint_without_inference() {
    // Given
    let config = parse_config(br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"file:///tmp/fixture"},"max_retries":3}"#);
    let provider = build_embedding_provider().unwrap();
    // When
    let result = provider.validate_config(&config);
    // Then
    assert!(result.is_err());
}

mod support;
use letopis::service::EmbeddingErrorKind;
use serde_json::{Value, json};
use std::{num::NonZeroUsize, process::Command};
use support::embedding::{FixtureReply, HttpFixture};

#[test]
fn sends_native_request_without_silent_truncation_or_unknown_parameters() {
    // Given
    let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3],[4,5,6]]}));
    let config = build_config(
        "ollama",
        &server.get_base_url(),
        &json!({"dimensions":3,"keep_alive":"5m","options":{"seed":7},"unknown":42,"input":"override","model":"override"}),
    );
    let provider = build_embedding_provider().unwrap();
    // When
    let vectors = provider
        .embed(&config, &["first", "second"], NonZeroUsize::new(3))
        .unwrap();
    // Then
    assert_eq!(vectors, vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    assert_eq!(
        server.take_requests(),
        vec![
            json!({"model":"fixture","input":["first","second"],"truncate":false,"dimensions":3,"keep_alive":"5m","options":{"seed":7}})
        ]
    );
    let headers = server.take_headers().pop().unwrap().to_lowercase();
    assert!(headers.starts_with("post /api/embed "));
    assert!(!headers.contains("authorization:"));
}
#[test]
fn reorders_compatible_response_and_sends_only_supported_parameters() {
    // Given: isolate synthetic credentials in a child process.
    if std::env::var_os("LETOPIS_PROVIDER_CHILD").is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "reorders_compatible_response_and_sends_only_supported_parameters",
            ])
            .env("LETOPIS_PROVIDER_CHILD", "1")
            .env("OPENAI_API_KEY", "fixture-secret")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let server = start_fixture(
        &json!({"data":[{"index":1,"embedding":[4,5,6]},{"index":0,"embedding":[1,2,3]}]}),
    );
    let config = build_config(
        "openai",
        &server.get_base_url(),
        &json!({"dimensions":3,"options":{},"unknown":99}),
    );
    let provider = build_embedding_provider().unwrap();
    // When
    let vectors = provider.embed(&config, &["first", "second"], None).unwrap();
    // Then
    assert_eq!(vectors, vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    assert_eq!(
        server.take_requests(),
        vec![
            json!({"model":"fixture","input":["first","second"],"encoding_format":"float","dimensions":3})
        ]
    );
    let headers = server.take_headers().pop().unwrap();
    assert!(headers.to_lowercase().starts_with("post /api/embeddings "));
    assert!(headers.contains("Bearer fixture-secret"));
}
#[test]
fn rejects_malformed_native_vectors() {
    // Given
    let bodies = [
        json!({"model":"fixture-alias","embeddings":[]}),
        json!({"model":"fixture-alias","embeddings":[[]]}),
        json!({"model":"fixture-alias","embeddings":[[1]]}),
        json!({"model":"fixture-alias","embeddings":[[1,2],[1,2,3]]}),
        json!({"model":"fixture-alias","embeddings":[[1,2,3e99]]}),
        json!({"model":"fixture-alias","embeddings":[[1,"secret",3]]}),
        json!({"error":"provider-secret"}),
        json!({"model":"fixture-alias","embeddings":[[1,2,3],[4,5,6]]}),
    ];
    for body in bodies {
        let server = start_fixture(&body);
        let config = build_config("ollama", &server.get_base_url(), &json!({"dimensions":3}));
        // When
        let error = build_embedding_provider()
            .unwrap()
            .embed(&config, &["input-secret"], None)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidResponse);
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
}
#[test]
fn rejects_non_json_and_non_success_without_retrying() {
    for (status, body, kind) in [
        (200, "not-json", EmbeddingErrorKind::InvalidResponse),
        (401, "secret", EmbeddingErrorKind::HttpStatus(401)),
        (201, "{}", EmbeddingErrorKind::HttpStatus(201)),
        (500, "secret", EmbeddingErrorKind::HttpStatus(500)),
        (
            200,
            "{\"embeddings\":[[NaN,2,3]]}",
            EmbeddingErrorKind::InvalidResponse,
        ),
    ] {
        // Given
        let server = HttpFixture::start(FixtureReply::Status {
            status,
            body: body.to_owned(),
        });
        let config = build_config("ollama", &server.get_base_url(), &json!({}));
        // When
        let error = build_embedding_provider()
            .unwrap()
            .embed(&config, &["input"], None)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), kind);
        assert_eq!(server.take_requests().len(), 1);
    }
}
#[test]
fn refuses_redirects() {
    // Given
    let destination = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3]]}));
    let source = HttpFixture::start(FixtureReply::Redirect {
        location: destination.get_base_url(),
    });
    let config = build_config("ollama", &source.get_base_url(), &json!({}));
    // When
    let error = build_embedding_provider()
        .unwrap()
        .embed(&config, &["input"], None)
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::HttpStatus(302));
    assert!(destination.take_requests().is_empty());
}

#[test]
fn changes_identity_when_retries_or_unknown_parameters_change() {
    // Given
    let first = json!({"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","future":1},"max_retries":3});
    let mut changed_retries = first.clone();
    changed_retries["max_retries"] = 2.into();
    let mut changed_parameter = first.clone();
    changed_parameter["parameters"]["future"] = 2.into();
    // When
    let original = parse_config(&serde_json::to_vec(&first).unwrap());
    let retries = parse_config(&serde_json::to_vec(&changed_retries).unwrap());
    let parameter = parse_config(&serde_json::to_vec(&changed_parameter).unwrap());
    // Then
    assert_ne!(original.get_identity(), retries.get_identity());
    assert_ne!(original.get_identity(), parameter.get_identity());
}
#[test]
fn canonicalizes_jcs_number_spellings() {
    for (first, second) in [("1.0", "1"), ("-0.0", "0"), ("1e30", "1E+30")] {
        // Given
        let build = |number| {
            format!(
                r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","future":{number}}},"max_retries":3}}"#
            )
        };
        // When
        let original = parse_config(build(first).as_bytes());
        let equivalent = parse_config(build(second).as_bytes());
        // Then
        assert_eq!(original.get_identity(), equivalent.get_identity());
    }
}
#[test]
fn rejects_missing_required_fields_and_forbidden_root_fields() {
    // Given
    let valid = json!({"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":3});
    for field in ["provider", "model", "parameters", "max_retries"] {
        let mut incomplete = valid.clone();
        incomplete.as_object_mut().unwrap().remove(field);
        // When / Then
        assert!(
            EmbeddingConfig::parse(&serde_json::to_vec(&incomplete).unwrap()).is_err(),
            "{field}"
        );
    }
    for field in ["api_key", "authorization", "unexpected"] {
        let mut forbidden = valid.clone();
        forbidden[field] = "secret".into();
        // When / Then
        assert!(
            EmbeddingConfig::parse(&serde_json::to_vec(&forbidden).unwrap()).is_err(),
            "{field}"
        );
    }
}
#[test]
fn rejects_wrong_recognized_parameter_types_and_credentials() {
    for (field, value) in [
        ("base_url", json!(7)),
        ("dimensions", json!("3")),
        ("dimensions", json!(0)),
        ("dimensions", json!(1)),
        ("dimensions", json!(4097)),
        ("dimensions", json!(-3)),
        ("dimensions", json!(3.5)),
        ("keep_alive", json!(2)),
        ("options", json!([])),
        ("truncate", json!(true)),
        ("encoding_format", json!("base64")),
        ("api_key", json!("secret")),
        ("Authorization", json!("secret")),
    ] {
        // Given
        let mut config = json!({"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":3});
        config["parameters"][field] = value;
        // When
        let result = EmbeddingConfig::parse(&serde_json::to_vec(&config).unwrap());
        // Then
        assert!(result.is_err(), "{field}");
    }
}
#[test]
fn rejects_duplicate_root_keys_and_duplicates_inside_arrays() {
    for bytes in [
        r#"{"provider":"ollama","model":"fixture","model":"other","parameters":{"base_url":"http://localhost/api"},"max_retries":3}"#,
        r#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","future":[{"a":1,"a":2}]},"max_retries":3}"#,
    ] {
        // Given / When
        let result = EmbeddingConfig::parse(bytes.as_bytes());
        // Then
        assert!(result.is_err());
    }
}
#[test]
fn rejects_overflow_and_canonicalization_budget_collisions() {
    for retries in [
        "18446744073709551615",
        "9223372036854775807",
        "9007199254740993",
        "-1",
        "1.5",
    ] {
        // Given
        let bytes = format!(
            r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api"}},"max_retries":{retries}}}"#
        );
        // When
        let result = EmbeddingConfig::parse(bytes.as_bytes());
        // Then
        assert!(result.is_err(), "{retries}");
    }
    let exact = parse_config(br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":9007199254740992}"#);
    assert_eq!(exact.get_max_attempts(), 9007199254740993);
}
#[test]
fn rejects_invalid_endpoint_schemes_userinfo_and_incomplete_hosts() {
    let provider = build_embedding_provider().unwrap();
    for endpoint in [
        "ftp://localhost/api",
        "http://user:secret@localhost/api",
        "http:///api",
        "localhost/api",
        "http://:123/api",
        "http://localhost:invalid/api",
    ] {
        // Given
        let config = build_config("ollama", endpoint, &json!({}));
        // When
        let result = provider.validate_config(&config);
        // Then
        assert!(result.is_err(), "{endpoint}");
    }
}
#[test]
fn rejects_compatible_index_and_vector_failures() {
    if run_isolated(
        "rejects_compatible_index_and_vector_failures",
        Some("fixture-secret"),
        &[],
    ) {
        return;
    }
    let entry = |index, vector| json!({"index":index,"embedding":vector});
    for data in [
        json!([]),
        json!([entry(0, json!([1, 2, 3]))]),
        json!([entry(0, json!([1, 2, 3])), entry(0, json!([1, 2, 3]))]),
        json!([entry(0, json!([1, 2, 3])), entry(2, json!([1, 2, 3]))]),
        json!([entry(-1, json!([1, 2, 3])), entry(1, json!([1, 2, 3]))]),
        json!([
            json!({"index":0.5,"embedding":[1,2,3]}),
            entry(1, json!([1, 2, 3]))
        ]),
        json!([{"embedding":[1,2,3]},entry(1,json!([1,2,3]))]),
        json!([entry(0, json!([])), entry(1, json!([1, 2, 3]))]),
        json!([
            entry(0, json!([1, "secret", 3])),
            entry(1, json!([1, 2, 3]))
        ]),
        json!([entry(0, json!([1, 2, 3e99])), entry(1, json!([1, 2, 3]))]),
        json!([entry(0, json!([1, 2])), entry(1, json!([1, 2, 3]))]),
    ] {
        // Given
        let server = start_fixture(&json!({"data":data}));
        let config = build_config("openai", &server.get_base_url(), &json!({"dimensions":3}));
        // When
        let error = build_embedding_provider()
            .unwrap()
            .embed(&config, &["first", "second"], None)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidResponse);
    }
}
#[test]
fn keeps_ollama_available_without_compatible_credentials() {
    if run_isolated(
        "keeps_ollama_available_without_compatible_credentials",
        None,
        &[],
    ) {
        return;
    }
    // Given
    let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3]]}));
    let compatible = build_config("openai", &server.get_base_url(), &json!({}));
    let native = build_config("ollama", &server.get_base_url(), &json!({}));
    let provider = build_embedding_provider().unwrap();
    // When
    provider.validate_config(&compatible).unwrap();
    let error = provider.embed(&compatible, &["input"], None).unwrap_err();
    let vectors = provider.embed(&native, &["input"], None).unwrap();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::Credentials);
    assert_eq!(vectors, vec![vec![1.0, 2.0, 3.0]]);
    assert_eq!(server.take_requests().len(), 1);
}
#[test]
fn rejects_invalid_compatible_credentials_before_network_io() {
    if run_isolated(
        "rejects_invalid_compatible_credentials_before_network_io",
        Some("fixture\nsecret"),
        &[],
    ) {
        return;
    }
    // Given
    let server = start_fixture(&json!({"data":[{"index":0,"embedding":[1,2,3]}]}));
    let config = build_config("openai", &server.get_base_url(), &json!({}));
    // When
    let error = build_embedding_provider()
        .unwrap()
        .embed(&config, &["input"], None)
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::Credentials);
    assert!(server.take_requests().is_empty());
}
#[test]
fn disables_environment_proxies_and_native_authorization() {
    if run_isolated(
        "disables_environment_proxies_and_native_authorization",
        Some("fixture-secret"),
        &[
            ("HTTP_PROXY", "http://127.0.0.1:1"),
            ("HTTPS_PROXY", "http://127.0.0.1:1"),
            ("ALL_PROXY", "http://127.0.0.1:1"),
            ("http_proxy", "http://127.0.0.1:1"),
            ("https_proxy", "http://127.0.0.1:1"),
            ("all_proxy", "http://127.0.0.1:1"),
            ("NO_PROXY", ""),
            ("no_proxy", ""),
        ],
    ) {
        return;
    }
    // Given
    let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3]]}));
    let config = build_config("ollama", &server.get_base_url(), &json!({}));
    // When
    let vectors = build_embedding_provider()
        .unwrap()
        .embed(&config, &["input"], None)
        .unwrap();
    // Then
    assert_eq!(vectors.len(), 1);
    assert!(
        !server.take_headers()[0]
            .to_lowercase()
            .contains("authorization:")
    );
}
#[test]
fn redacts_cause_bearing_errors_and_config_debug() {
    use letopis::service::EmbeddingError;
    use std::error::Error;
    // Given
    let error = EmbeddingError::new_with_cause(
        EmbeddingErrorKind::InvalidResponse,
        std::io::Error::other("provider-secret Authorization: fixture-secret"),
    );
    let config = build_config(
        "ollama",
        "http://localhost/api",
        &json!({"future":"knowledge-secret"}),
    );
    // When
    let debug = format!("{error:?}");
    let config_debug = format!("{config:?}");
    let formatted = format!("{error} {debug} {config_debug}");
    let source = error.source().unwrap();
    // Then
    assert!(debug.contains(&format!("{:?}", EmbeddingErrorKind::InvalidResponse)));
    assert!(config_debug.contains(&format!("{:?}", config.get_provider())));
    assert!(!formatted.contains("secret"));
    assert!(!format!("{source} {source:?}").contains("secret"));
    assert!(source.source().is_none());
}
#[test]
fn rejects_empty_inputs_and_incompatible_requested_dimensions_without_io() {
    // Given
    let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3]]}));
    let config = build_config("ollama", &server.get_base_url(), &json!({"dimensions":3}));
    let provider = build_embedding_provider().unwrap();
    // When / Then
    for (texts, dimensions) in [
        (vec![], None),
        (vec![""], None),
        (vec!["input"], NonZeroUsize::new(2)),
        (vec!["input"], NonZeroUsize::new(4097)),
    ] {
        let error = provider.embed(&config, &texts, dimensions).unwrap_err();
        assert_eq!(error.get_kind(), EmbeddingErrorKind::Input);
    }
    assert!(server.take_requests().is_empty());
}
#[test]
fn rejects_connection_failures_without_exposing_endpoint_or_input() {
    // Given
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/secret", listener.local_addr().unwrap());
    drop(listener);
    let config = build_config("ollama", &url, &json!({}));
    // When
    let error = build_embedding_provider()
        .unwrap()
        .embed(&config, &["knowledge-secret"], None)
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::Connection);
    assert!(!format!("{error} {error:?}").contains("secret"));
}
#[test]
#[ignore = "Task 5 executes the real 60-second acceptance deadline once per platform"]
fn attempt_obeys_real_sixty_second_deadline() {
    use std::time::{Duration, Instant};
    // Given
    let server = HttpFixture::start(FixtureReply::DelayedBody {
        delay: Duration::from_secs(65),
        body: json!({"model":"fixture-alias","embeddings":[[1,2,3]]}).to_string(),
    });
    let config = build_config("ollama", &server.get_base_url(), &json!({}));
    let start = Instant::now();
    // When
    let error = build_embedding_provider()
        .unwrap()
        .embed(&config, &["input"], None)
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::Timeout);
    assert!((Duration::from_secs(59)..Duration::from_secs(64)).contains(&start.elapsed()));
    assert_eq!(server.take_requests().len(), 1);
}

#[test]
fn rejects_compatible_error_payload_status_and_non_json() {
    if run_isolated(
        "rejects_compatible_error_payload_status_and_non_json",
        Some("fixture-secret"),
        &[],
    ) {
        return;
    }
    for (status, body, kind) in [
        (
            200,
            r#"{"error":{"message":"provider-secret"}}"#,
            EmbeddingErrorKind::InvalidResponse,
        ),
        (200, "not-json-secret", EmbeddingErrorKind::InvalidResponse),
        (
            200,
            r#"{"data":[{"index":0,"embedding":[1,Infinity,3]}]}"#,
            EmbeddingErrorKind::InvalidResponse,
        ),
        (401, "provider-secret", EmbeddingErrorKind::HttpStatus(401)),
    ] {
        // Given
        let server = HttpFixture::start(FixtureReply::Status {
            status,
            body: body.to_owned(),
        });
        let config = build_config("openai", &server.get_base_url(), &json!({}));
        // When
        let error = build_embedding_provider()
            .unwrap()
            .embed(&config, &["knowledge-secret"], None)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), kind);
        assert!(!format!("{error} {error:?}").contains("secret"));
        assert_eq!(server.take_requests().len(), 1);
    }
}
#[test]
fn validates_unconfigured_dimensions_without_sending_an_implicit_request_dimension() {
    for size in [2, 4096] {
        // Given
        let vector = vec![0.5; size];
        let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[vector]}));
        let config = build_config("ollama", &server.get_base_url(), &json!({}));
        // When
        let vectors = build_embedding_provider()
            .unwrap()
            .embed(&config, &["input"], NonZeroUsize::new(size))
            .unwrap();
        // Then
        assert_eq!(vectors[0], vector);
        assert!(server.take_requests()[0].get("dimensions").is_none());
    }
}
#[test]
fn rejects_inconsistent_unconfigured_and_unsupported_response_dimensions() {
    for embeddings in [
        json!([[1, 2], [1, 2, 3]]),
        json!([vec![0.0; 4097], vec![0.0; 4097]]),
    ] {
        // Given
        let server = start_fixture(&json!({"model":"fixture-alias","embeddings":embeddings}));
        let config = build_config("ollama", &server.get_base_url(), &json!({}));
        // When
        let error = build_embedding_provider()
            .unwrap()
            .embed(&config, &["first", "second"], None)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidResponse);
    }
}
#[test]
fn appends_endpoint_before_query_and_ignores_fragment() {
    // Given
    let server = start_fixture(&json!({"model":"fixture-alias","embeddings":[[1,2,3]]}));
    let endpoint = format!("{}/?fixture=1#ignored", server.get_base_url());
    let config = build_config("ollama", &endpoint, &json!({}));
    // When
    build_embedding_provider()
        .unwrap()
        .embed(&config, &["input"], None)
        .unwrap();
    // Then
    assert!(server.take_headers()[0].starts_with("POST /api/embed?fixture=1 "));
}
#[test]
fn controlled_barrier_releases_an_owned_request() {
    use support::embedding::ResponseBarrier;
    // Given
    let barrier = ResponseBarrier::default();
    let server = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: json!({"model":"fixture-alias","embeddings":[[1,2,3]]}).to_string(),
    });
    let config = build_config("ollama", &server.get_base_url(), &json!({}));
    let provider = build_embedding_provider().unwrap();
    // When
    let worker = std::thread::spawn(move || provider.embed(&config, &["input"], None));
    barrier.wait_until_entered();
    let requests = server.take_requests();
    barrier.release();
    let vectors = worker.join().unwrap().unwrap();
    // Then
    assert_eq!(requests.len(), 1);
    assert_eq!(vectors, vec![vec![1.0, 2.0, 3.0]]);
}

#[test]
fn rejects_adjacent_nested_option_integers_that_alias_under_jcs() {
    // Given
    let exact = build_config(
        "ollama",
        "http://localhost/api",
        &json!({"options":{"seed":9007199254740992_u64}}),
    );
    let distinct = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","options":{"seed":9007199254740993}},"max_retries":3}"#;
    // When
    let result = EmbeddingConfig::parse(distinct);
    // Then
    assert_eq!(
        exact.get_parameters()["options"]["seed"].as_u64(),
        Some(9007199254740992)
    );
    assert!(result.is_err());
}
#[test]
fn preserves_exact_large_option_integers_and_rejects_nested_array_rounding() {
    // Given
    let exact = build_config(
        "ollama",
        "http://localhost/api",
        &json!({"options":{"seed":9007199254740994_u64}}),
    );
    let negative = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","future":[{"counter":-9007199254740993}]},"max_retries":3}"#;
    // When
    let result = EmbeddingConfig::parse(negative);
    // Then
    assert_eq!(
        exact.get_parameters()["options"]["seed"].as_u64(),
        Some(9007199254740994)
    );
    assert!(result.is_err());
}

#[test]
fn rejects_integer_tokens_above_unsigned_range_before_rounding() {
    for integer in [
        "18446744073709551616",
        "18446744073709551617",
        "18446744073709551618",
    ] {
        // Given
        let bytes = format!(
            r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","options":{{"seed":{integer}}}}},"max_retries":3}}"#
        );
        // When
        let result = EmbeddingConfig::parse(bytes.as_bytes());
        // Then
        let error = result.expect_err("overflowing integer token must remain detectable");
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidConfig);
        assert!(!format!("{error} {error:?}").contains(integer));
    }
}
#[test]
fn rejects_integer_tokens_below_signed_range_inside_nested_arrays() {
    for integer in ["-9223372036854775809", "-9223372036854775810"] {
        // Given
        let bytes = format!(
            r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","options":{{"future":[{{"seed":{integer}}}]}}}},"max_retries":3}}"#
        );
        // When
        let result = EmbeddingConfig::parse(bytes.as_bytes());
        // Then
        let error = result.expect_err("overflowing integer token must remain detectable");
        assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidConfig);
        assert!(!format!("{error} {error:?}").contains(integer));
    }
}

#[test]
fn preserves_numeric_strings_and_large_noninteger_spellings() {
    // Given
    let first = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","options":{"text":"18446744073709551617","scale":1e30}},"max_retries":3}"#;
    let second = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","options":{"text":"18446744073709551617","scale":1E+30}},"max_retries":3}"#;
    // When
    let original = parse_config(first);
    let equivalent = parse_config(second);
    // Then
    assert_eq!(original.get_identity(), equivalent.get_identity());
    assert_eq!(
        original.get_parameters()["options"]["text"],
        "18446744073709551617"
    );
}

#[test]
fn retains_bounded_json_nesting_while_preserving_raw_integer_tokens() {
    // Given
    let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
    let bytes = format!(
        r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","future":{nested}}},"max_retries":3}}"#
    );
    // When
    let result = EmbeddingConfig::parse(bytes.as_bytes());
    // Then
    assert!(result.is_err());
}

#[test]
fn preserves_reserved_key_objects_in_unknown_config_parameters() {
    // Given
    let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","future":{"$serde_json::private::RawValue":"null"}},"max_retries":0}"#;
    let reordered = br#"{"max_retries":0,"parameters":{"future":{"$serde_json::private::RawValue":"null"},"base_url":"http://localhost/api"},"model":"fixture","provider":"ollama"}"#;
    // When
    let first = parse_config(bytes);
    let second = parse_config(reordered);
    // Then
    assert_eq!(
        first.get_parameters()["future"],
        json!({"$serde_json::private::RawValue":"null"})
    );
    assert_eq!(first.get_identity(), second.get_identity());
    let changed = parse_config(br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","future":null},"max_retries":0}"#);
    assert_ne!(first.get_identity(), changed.get_identity());
}

#[test]
fn preserves_decimal_and_exponent_values_with_large_integer_jcs_spelling() {
    for number in ["18446744073709551616.0", "1e20", "-1e20"] {
        // Given
        let bytes = format!(
            r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","future":{number}}},"max_retries":0}}"#
        );
        // When
        let config = parse_config(bytes.as_bytes());
        // Then
        assert_eq!(
            config.get_parameters()["future"].as_f64(),
            number.parse::<f64>().ok()
        );
    }
}

#[test]
fn rejects_object_wire_components_indices_and_wrapped_responses() {
    // Given
    if run_isolated(
        "rejects_object_wire_components_indices_and_wrapped_responses",
        Some("fixture-secret"),
        &[],
    ) {
        return;
    }
    let object = json!({"$serde_json::private::RawValue":"1"});
    for protocol in ["ollama", "openai"] {
        let valid = build_response(protocol);
        let mut component = valid.clone();
        if protocol == "ollama" {
            component["embeddings"][0][0] = object.clone();
        } else {
            component["data"][0]["embedding"][0] = object.clone();
        }
        let wrapped = json!({"$serde_json::private::RawValue":valid.to_string()});
        let mut invalid = vec![component, wrapped];
        if protocol == "openai" {
            let mut index = valid;
            index["data"][0]["index"] = json!({"$serde_json::private::RawValue":"0"});
            invalid.push(index);
        }
        for body in invalid {
            // When / Then
            assert_rejected_response(
                protocol,
                FixtureReply::Status {
                    status: 200,
                    body: body.to_string(),
                },
            );
        }
    }
}

#[test]
fn bounds_both_protocol_bodies_with_and_without_content_length() {
    if run_isolated(
        "bounds_both_protocol_bodies_with_and_without_content_length",
        Some("fixture-secret"),
        &[],
    ) {
        return;
    }
    for protocol in ["ollama", "openai"] {
        // Given: valid protocol payload with a large unknown field.
        let mut body = build_response(protocol);
        body["extra"] = format!("provider-secret{}", "x".repeat(10 * 1024 * 1024)).into();
        let body = body.to_string();
        for reply in [
            FixtureReply::Status {
                status: 200,
                body: body.clone(),
            },
            FixtureReply::CloseDelimited { body: body.clone() },
        ] {
            assert_rejected_response(protocol, reply);
        }
        // When: a normal bounded response still succeeds, including model aliases.
        let server = start_fixture(&build_response(protocol));
        let config = build_config(protocol, &server.get_base_url(), &json!({}));
        let vectors = build_embedding_provider()
            .unwrap()
            .embed(&config, &["input"], None)
            .unwrap();
        // Then
        assert_eq!(vectors, vec![vec![1., 2., 3.]]);
        assert_eq!(server.take_requests().len(), 1);
    }
}

#[test]
fn rejects_missing_empty_and_wrong_type_native_model() {
    // Given
    for model in [
        None,
        Some(json!("")),
        Some(json!(1)),
        Some(json!(null)),
        Some(json!({"$serde_json::private::RawValue":"\"fixture\""})),
    ] {
        let mut body = build_response("ollama");
        body.as_object_mut().unwrap().remove("model");
        if let Some(model) = model {
            body["model"] = model;
        }
        // When / Then
        assert_rejected_response(
            "ollama",
            FixtureReply::Status {
                status: 200,
                body: body.to_string(),
            },
        );
    }
}

#[test]
fn rejects_trailing_response_tokens_and_excessive_depth() {
    // Given
    let valid = build_response("ollama").to_string();
    let deep = format!("{}0{}", "[".repeat(130), "]".repeat(130));
    for body in [
        format!("{valid} null"),
        format!(r#"{{"model":"fixture","embeddings":[[1,2,3]],"extra":{deep}}}"#),
    ] {
        // When / Then
        assert_rejected_response("ollama", FixtureReply::Status { status: 200, body });
    }
}

fn start_fixture(body: &Value) -> HttpFixture {
    HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: body.to_string(),
    })
}

fn run_isolated(name: &str, key: Option<&str>, extras: &[(&str, &str)]) -> bool {
    if std::env::var_os("LETOPIS_PROVIDER_CHILD").is_some() {
        return false;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name])
        .env("LETOPIS_PROVIDER_CHILD", "1")
        .env_remove("OPENAI_API_KEY");
    if let Some(key) = key {
        command.env("OPENAI_API_KEY", key);
    }
    for (name, value) in extras {
        command.env(name, value);
    }
    assert!(command.status().unwrap().success());
    true
}

fn build_response(protocol: &str) -> Value {
    if protocol == "ollama" {
        json!({"model":"fixture-alias","embeddings":[[1,2,3]]})
    } else {
        json!({"data":[{"index":0,"embedding":[1,2,3]}]})
    }
}

fn assert_rejected_response(protocol: &str, reply: FixtureReply) {
    // Given
    let server = HttpFixture::start(reply);
    let config = build_config(protocol, &server.get_base_url(), &json!({}));
    // When
    let error = build_embedding_provider()
        .unwrap()
        .embed(&config, &["knowledge-secret"], None)
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), EmbeddingErrorKind::InvalidResponse);
    assert!(!format!("{error} {error:?}").contains("secret"));
    assert_eq!(server.take_requests().len(), 1);
}

fn build_config(provider: &str, url: &str, extras: &Value) -> EmbeddingConfig {
    let mut parameters = extras.as_object().unwrap().clone();
    parameters.insert("base_url".to_owned(), url.into());
    parse_config(
        &serde_json::to_vec(&json!({"provider": provider, "model":"fixture",
        "parameters": parameters, "max_retries":3}))
        .unwrap(),
    )
}

fn parse_config(bytes: &[u8]) -> EmbeddingConfig {
    EmbeddingConfig::parse(bytes).unwrap()
}
