use super::http::{Http, build_endpoint, decode_vectors, prepare_body, validate_config};
use crate::service::{
    EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult,
    ProviderKind,
};
use serde_json::Value;
use std::{fmt, num::NonZeroUsize};

pub(crate) struct Openai {
    http: Http,
    key: Option<String>,
}
impl Openai {
    pub(crate) fn new(key: Option<String>) -> Self {
        Self {
            http: Http::new(),
            key,
        }
    }
    fn build_authorization(&self) -> EmbeddingResult<String> {
        let key = self
            .key
            .as_deref()
            .filter(|key| !key.is_empty() && key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::Credentials))?;
        Ok(format!("Bearer {key}"))
    }
}
impl fmt::Debug for Openai {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Openai { .. }")
    }
}
impl EmbeddingProvider for Openai {
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
        validate_config(config, ProviderKind::Openai)
    }
    fn embed(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        let endpoint = build_endpoint(config, ProviderKind::Openai, "embeddings")?;
        let mut body = prepare_body(config, texts, dimensions)?;
        body["encoding_format"] = "float".into();
        let authorization = self.build_authorization()?;
        let response = self.http.post(&endpoint, &body, Some(&authorization))?;
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
}
