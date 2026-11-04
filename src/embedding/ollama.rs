use super::http::{Http, build_endpoint, decode_vectors, prepare_body};
use crate::service::{
    EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingResult, ProviderKind,
};
use serde_json::Value;
use std::num::NonZeroUsize;

/// Compute native vectors without credentials, requiring untruncated input and a model response.
///
/// # Errors
/// Reports invalid configuration/input, transport failure or malformed native vectors.
pub(super) fn vectors_compute_by_texts(
    http: &Http,
    config: &EmbeddingConfig,
    texts: &[&str],
    dimensions: Option<NonZeroUsize>,
) -> EmbeddingResult<Vec<Vec<f32>>> {
    let endpoint = build_endpoint(config, ProviderKind::Ollama, "embed")?;
    let mut body = prepare_body(config, texts, dimensions)?;
    body["truncate"] = false.into();
    for key in ["keep_alive", "options"] {
        if let Some(value) = config.get_parameters().get(key) {
            body[key] = value.clone();
        }
    }
    let response = http.request_send_by_endpoint(&endpoint, &body, None)?;
    response
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
    let items = response
        .get("embeddings")
        .and_then(Value::as_array)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
    decode_vectors(
        items,
        texts.len(),
        dimensions.or(config.get_dimensions()),
        ProviderKind::Ollama,
    )
}
