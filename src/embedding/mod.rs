//! Concrete synchronous embedding protocols.
mod http;
mod ollama;
mod openai;
mod provider;
pub(crate) use provider::HttpProviders;
