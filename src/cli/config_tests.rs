use super::{CommandResponse, Request, ResponseResult, request_parse, response_write};
use crate::service::{CommandErrorKind, Configuration};
use serde_json::{Value, json};
use std::{ffi::OsString, io::Read};

const LITERAL_CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}}"#;

#[test]
fn rejects_nonempty_config_request_before_io() {
    // Given: invalid argv must fail before stdin or any service is read.
    let mut unread = UnreadInput;

    // When: the config argv includes an unsupported positional argument.
    let error = request_parse(
        [OsString::from("config"), OsString::from("extra")].as_slice(),
        &mut unread,
    )
    .unwrap_err();

    // Then: validation fails before the unreadable input is touched.
    assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);

    // Given: malformed or nonempty configuration request bodies.
    for body in [
        "null",
        "[]",
        r#"{"provider":"ollama"}"#,
        r#"{"extra":1}"#,
        r#"{"extra":1,"extra":2}"#,
        "{} {}",
    ] {
        // When: each body is parsed as a config request.
        let error =
            request_parse([OsString::from("config")].as_slice(), body.as_bytes()).unwrap_err();

        // Then: each unsupported body returns invalid_request.
        assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest, "{body}");
    }

    // Given / When / Then: an empty JSON object parses as the config request.
    assert!(matches!(
        request_parse([OsString::from("config")].as_slice(), b"{}".as_slice()).unwrap(),
        Request::Config
    ));
}

#[test]
fn renders_exact_contract_object_without_warnings() {
    // Given: a configuration with literal public fields and no warnings.
    let config = Configuration::parse(LITERAL_CONFIG).unwrap();
    let response = Ok(CommandResponse::new(ResponseResult::Config(Some(config))));

    // When: the configuration response is written to a buffer.
    let mut output = Vec::new();
    let exit = response_write(&mut output, response);

    // Then: one successful JSON envelope contains exactly the public configuration.
    assert_eq!(exit, 0);
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value,
        json!({"ok":true,"result":{"embedding_model":{
            "enabled":true,
            "provider":"ollama",
            "model":"fixture",
            "parameters":{
                "base_url":"http://127.0.0.1:9",
                "custom":{"enabled":true},
                "dimensions":3
            },
            "max_retries":2
        }}})
    );
    assert!(value.get("warnings").is_none());
    assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
}

struct UnreadInput;

impl Read for UnreadInput {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("stdin must not be read before argv validation");
    }
}
