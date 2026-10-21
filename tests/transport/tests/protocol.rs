use letopis_transport_checks::protocol::{Config, Provider, decode, prepare};
use serde_json::{Value, json};

fn configuration(provider: &str) -> Value {
    json!({"provider":provider,"model":"fixture-model","parameters":{"base_url":"http://localhost:12345/custom/api?route=fixture#fragment"},"max_retries":2})
}
fn parse(value: &Value) -> Config {
    Config::parse(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn texts() -> Vec<String> {
    vec!["alpha".into(), "βeta".into()]
}

#[test]
fn native_request_is_literal_and_cannot_be_overridden() {
    let mut c = configuration("ollama");
    c["parameters"].as_object_mut().unwrap().extend(json!({"dimensions":3,"keep_alive":"5m","options":{"seed":42},"model":"other","input":"wrong","truncate":true,"unknown":{"kept":true}}).as_object().unwrap().clone());
    let config = parse(&c);
    assert_eq!(config.parameters()["unknown"], json!({"kept":true}));
    let r = prepare(&config, &texts(), Some("KEY_CANARY")).unwrap();
    let (url, body, auth) = r.parts();
    assert_eq!(url, "http://localhost:12345/custom/api/embed?route=fixture");
    assert_eq!(
        body,
        &json!({"model":"fixture-model","input":["alpha","βeta"],"truncate":false,"dimensions":3,"keep_alive":"5m","options":{"seed":42}})
    );
    assert_eq!(auth, None);
    assert!(!format!("{r:?}").contains("alpha"));
}

#[test]
fn compatible_request_is_literal_and_requires_safe_bearer() {
    let mut c = configuration("openai");
    c["parameters"]["dimensions"] = json!(3);
    let config = parse(&c);
    let r = prepare(&config, &texts(), Some("KEY_CANARY")).unwrap();
    let (url, body, auth) = r.parts();
    assert_eq!(
        url,
        "http://localhost:12345/custom/api/embeddings?route=fixture"
    );
    assert_eq!(
        body,
        &json!({"model":"fixture-model","input":["alpha","βeta"],"encoding_format":"float","dimensions":3})
    );
    assert_eq!(auth, Some("Bearer KEY_CANARY"));
    for key in [None, Some(""), Some("bad\r\nkey"), Some("bad\0key")] {
        assert!(prepare(&config, &texts(), key).is_err());
    }
    assert!(!format!("{r:?}").contains("KEY_CANARY"));
}

#[test]
fn strict_configuration_rejects_invalid_shapes_without_defaulting() {
    let c = configuration("ollama");
    for field in ["provider", "model", "parameters", "max_retries"] {
        let mut missing = c.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            Config::parse(&serde_json::to_vec(&missing).unwrap()).is_err(),
            "missing {field}"
        );
        let mut null = c.clone();
        null[field] = Value::Null;
        assert!(
            Config::parse(&serde_json::to_vec(&null).unwrap()).is_err(),
            "null {field}"
        );
    }
    for (field, value) in [
        ("provider", json!("unknown")),
        ("model", json!("")),
        ("parameters", json!([])),
        ("max_retries", json!(-1)),
        ("max_retries", json!(1.5)),
        ("extra", json!(true)),
    ] {
        let mut bad = c.clone();
        bad[field] = value;
        assert!(
            Config::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{bad}"
        );
    }
    for url in [
        "ftp://localhost/a",
        "http://user:secret@localhost/api",
        "http:///",
        "relative",
    ] {
        let mut bad = c.clone();
        bad["parameters"]["base_url"] = json!(url);
        assert!(Config::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    for key in ["api_key", "authorization", "Authorization"] {
        let mut bad = c.clone();
        bad["parameters"][key] = json!("SECRET_CANARY");
        assert!(Config::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut bad = c.clone();
    bad["parameters"] = json!({});
    assert!(Config::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    assert!(Config::parse(b"null").is_err());
}

#[test]
fn request_validation_rejects_empty_input_and_wrong_documented_types() {
    let config = parse(&configuration("ollama"));
    assert!(prepare(&config, &[], None).is_err());
    assert!(prepare(&config, &[String::new()], None).is_err());
    for (key, value) in [
        ("dimensions", json!(0)),
        ("dimensions", json!(1.5)),
        ("keep_alive", json!([])),
        ("options", json!(true)),
    ] {
        let mut bad = configuration("ollama");
        bad["parameters"][key] = value;
        assert!(prepare(&parse(&bad), &texts(), None).is_err());
    }
}

#[test]
fn api_prefix_appends_exactly_one_separator() {
    for (base, expected) in [
        ("http://localhost:12345", "http://localhost:12345/embed"),
        ("http://localhost:12345/", "http://localhost:12345/embed"),
        (
            "http://localhost:12345/api/",
            "http://localhost:12345/api/embed",
        ),
    ] {
        let mut c = configuration("ollama");
        c["parameters"]["base_url"] = json!(base);
        let request = prepare(&parse(&c), &texts(), None).unwrap();
        assert_eq!(request.parts().0, expected);
    }
}

#[test]
fn response_order_and_optional_fields_follow_each_protocol() {
    let native = br#"{"model":"actual-model","embeddings":[[1,2,3],[4,5,6]],"extra":true}"#;
    assert_eq!(
        decode(Provider::Ollama, native, 2, 3).unwrap(),
        vec![vec![1., 2., 3.], vec![4., 5., 6.]]
    );
    let compatible =
        br#"{"data":[{"index":1,"embedding":[4,5,6]},{"index":0,"embedding":[1,2,3]}]}"#;
    assert_eq!(
        decode(Provider::Openai, compatible, 2, 3).unwrap(),
        vec![vec![1., 2., 3.], vec![4., 5., 6.]]
    );
}

#[test]
fn malformed_native_vectors_are_never_success() {
    for body in [
        "{}",
        "not-json",
        r#"{"error":"SECRET_CANARY"}"#,
        r#"{"model":"","embeddings":[[1,2,3],[4,5,6]]}"#,
        r#"{"embeddings":[[1,2,3],[4,5,6]]}"#,
    ] {
        assert!(decode(Provider::Ollama, body.as_bytes(), 2, 3).is_err());
    }
    for vectors in [
        json!([]),
        json!([[1, 2, 3]]),
        json!([[1, 2, 3], [4, 5]]),
        json!([[], []]),
        json!([[true, 2, 3], [4, 5, 6]]),
        json!([[null, 2, 3], [4, 5, 6]]),
        json!([["1", 2, 3], [4, 5, 6]]),
        json!([[1e100, 2, 3], [4, 5, 6]]),
    ] {
        let body = serde_json::to_vec(&json!({"model":"m","embeddings":vectors})).unwrap();
        let error = decode(Provider::Ollama, &body, 2, 3).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("SECRET_CANARY"));
    }
    for value in ["NaN", "Infinity", "1e400"] {
        let body = format!(r#"{{"model":"m","embeddings":[[{value},2,3],[4,5,6]]}}"#);
        assert!(decode(Provider::Ollama, body.as_bytes(), 2, 3).is_err());
    }
    assert!(decode(Provider::Ollama, br#"{"model":"m","embeddings":[]}"#, 0, 0).is_err());
}

#[test]
fn compatible_indices_must_bijectively_cover_inputs() {
    for data in [
        json!([]),
        json!([{"index":0,"embedding":[1,2,3]},{"index":0,"embedding":[4,5,6]}]),
        json!([{"index":0,"embedding":[1,2,3]},{"index":2,"embedding":[4,5,6]}]),
        json!([{"index":-1,"embedding":[1,2,3]},{"index":1,"embedding":[4,5,6]}]),
        json!([{"index":0.5,"embedding":[1,2,3]},{"index":1,"embedding":[4,5,6]}]),
        json!([{"embedding":[1,2,3]},{"index":1,"embedding":[4,5,6]}]),
        json!([{"index":0},{"index":1,"embedding":[4,5,6]}]),
    ] {
        assert!(
            decode(
                Provider::Openai,
                &serde_json::to_vec(&json!({"data":data})).unwrap(),
                2,
                3
            )
            .is_err()
        );
    }
}
