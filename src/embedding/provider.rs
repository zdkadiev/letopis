use super::{Ollama, Openai};
use crate::service::{EmbeddingConfig, EmbeddingProvider, EmbeddingResult, ProviderKind};
use std::num::NonZeroUsize;

pub(crate) struct HttpProviders {
    ollama: Ollama,
    openai: Openai,
}
impl HttpProviders {
    pub(crate) fn new(ollama: Ollama, openai: Openai) -> Self {
        Self { ollama, openai }
    }
}
impl EmbeddingProvider for HttpProviders {
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
        match config.get_provider() {
            ProviderKind::Ollama => self.ollama.validate_config(config),
            ProviderKind::Openai => self.openai.validate_config(config),
        }
    }
    fn embed(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        match config.get_provider() {
            ProviderKind::Ollama => self.ollama.embed(config, texts, dimensions),
            ProviderKind::Openai => self.openai.embed(config, texts, dimensions),
        }
    }
}
