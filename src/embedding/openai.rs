use super::http::{Http, build_endpoint, decode_vectors, prepare_body};
use crate::service::{
    EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingResult, ProviderKind,
};
use serde_json::Value;
use std::num::NonZeroUsize;

/// Compute compatible vectors with validated credentials and restore their input order.
///
/// # Errors
/// Reports invalid configuration/input, credentials, transport failure or malformed indexed vectors.
pub(super) fn vectors_compute_by_texts(
    http: &Http,
    key: Option<&str>,
    config: &EmbeddingConfig,
    texts: &[&str],
    dimensions: Option<NonZeroUsize>,
) -> EmbeddingResult<Vec<Vec<f32>>> {
    let endpoint = build_endpoint(config, ProviderKind::Openai, "embeddings")?;
    let mut body = prepare_body(config, texts, dimensions)?;
    body["encoding_format"] = "float".into();
    let authorization = build_authorization(key)?;
    let response = http.request_send_by_endpoint(&endpoint, &body, Some(&authorization))?;
    let items = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
    decode_vectors(
        items,
        texts.len(),
        dimensions.or(config.get_dimensions()),
        ProviderKind::Openai,
    )
}

fn build_authorization(key: Option<&str>) -> EmbeddingResult<String> {
    let key = key
        .filter(|key| !key.is_empty() && key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::Credentials))?;
    Ok(format!("Bearer {key}"))
}

#[cfg(test)]
mod tests {
    use super::build_authorization;
    use crate::service::EmbeddingErrorKind;

    #[test]
    fn accepts_only_nonempty_printable_ascii_credentials() {
        // Given: boundary characters are valid, while absent, empty and nonprintable keys are not.
        for (key, expected) in [
            (Some("!fixture~"), Some("Bearer !fixture~")),
            (None, None),
            (Some(""), None),
            (Some("fixture secret"), None),
            (Some("fixture\nsecret"), None),
            (Some("fixture\u{7f}secret"), None),
            (Some("fixture\u{80}secret"), None),
        ] {
            // When: compatible authorization is prepared without sending a request.
            let result = build_authorization(key);
            // Then: only the independently specified printable key produces a header.
            match expected {
                Some(header) => assert_eq!(result.unwrap(), header),
                None => {
                    let error = result.unwrap_err();
                    assert_eq!(error.get_kind(), EmbeddingErrorKind::Credentials);
                    assert!(!format!("{error} {error:?}").contains("secret"));
                }
            }
        }
    }
}
