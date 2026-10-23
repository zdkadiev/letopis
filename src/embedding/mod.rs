//! Concrete synchronous embedding protocols.
mod http;
mod ollama;
mod openai;
mod provider;
pub(crate) use ollama::Ollama;
pub(crate) use openai::Openai;
pub(crate) use provider::HttpProviders;
