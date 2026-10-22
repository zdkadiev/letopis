use super::http::{Http, build_endpoint, decode_vectors, invalid_response, prepare_body};
use crate::service::{EmbeddingConfig, EmbeddingProvider, EmbeddingResult, ProviderKind};
use serde_json::Value;
use std::num::NonZeroUsize;

pub(crate) struct Ollama {
    http: Http,
}
impl Ollama {
    pub(crate) fn new() -> Self {
        Self { http: Http::new() }
    }
}
impl EmbeddingProvider for Ollama {
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
        build_endpoint(config, ProviderKind::Ollama, "embed").map(|_| ())
    }
    fn embed(
        &self,
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
        let response = self.http.post(&endpoint, &body, None)?;
        let items = response
            .get("embeddings")
            .and_then(Value::as_array)
            .ok_or_else(invalid_response)?;
        decode_vectors(
            items,
            texts.len(),
            dimensions.or(config.get_dimensions()),
            ProviderKind::Ollama,
        )
    }
}
