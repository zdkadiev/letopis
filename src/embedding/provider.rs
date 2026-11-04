use super::{
    http::{Http, config_validate},
    ollama, openai,
};
use crate::service::{EmbeddingConfig, EmbeddingProvider, EmbeddingResult, ProviderKind};
use std::{fmt, num::NonZeroUsize};

/// Synchronous native and compatible protocols with independently owned HTTP pools.
pub(crate) struct HttpProviders {
    ollama_http: Http,
    openai_http: Http,
    key: Option<String>,
}
impl HttpProviders {
    /// Capture compatible credentials and construct a separate HTTP agent for each protocol.
    pub(crate) fn new(key: Option<String>) -> Self {
        Self {
            ollama_http: Http::new(),
            openai_http: Http::new(),
            key,
        }
    }
}
impl fmt::Debug for HttpProviders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpProviders { .. }")
    }
}
impl EmbeddingProvider for HttpProviders {
    fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
        config_validate(config, config.get_provider())
    }
    fn vectors_compute_by_texts(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        match config.get_provider() {
            ProviderKind::Ollama => {
                ollama::vectors_compute_by_texts(&self.ollama_http, config, texts, dimensions)
            }
            ProviderKind::Openai => openai::vectors_compute_by_texts(
                &self.openai_http,
                self.key.as_deref(),
                config,
                texts,
                dimensions,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HttpProviders;

    #[test]
    fn redacts_captured_credentials_from_owner_debug() {
        // Given: the real provider owner captures a synthetic compatible credential.
        let provider = HttpProviders::new(Some("owner-debug-secret".to_owned()));
        // When: both ordinary and pretty Debug formatting observe the owner.
        let ordinary = format!("{provider:?}");
        let pretty = format!("{provider:#?}");
        // Then: the literal representation contains no credential or transport internals.
        assert_eq!(ordinary, "HttpProviders { .. }");
        assert_eq!(pretty, "HttpProviders { .. }");
    }
}
