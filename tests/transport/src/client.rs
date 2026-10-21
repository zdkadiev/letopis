//! Blocking HTTP adapter; one attempt, no application retries or HTTP worker.
use crate::protocol::{Config, TransportError, decode, prepare};
use std::time::Duration;
use ureq::{
    Agent,
    tls::{RootCerts, TlsConfig},
};

pub struct Client {
    agent: Agent,
}

impl Client {
    pub fn new() -> Result<Self, TransportError> {
        Self::with_fixture_policy(Duration::from_secs(60))
    }

    /// Harness-only shorter deadline; uses the same routing and system TLS policy.
    pub fn with_fixture_policy(deadline: Duration) -> Result<Self, TransportError> {
        if deadline.is_zero() || deadline > Duration::from_secs(60) {
            return Err(TransportError::Config);
        }
        let agent = Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(deadline))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .new_agent();
        Ok(Self { agent })
    }

    pub fn embed(
        &self,
        config: &Config,
        texts: &[String],
        expected_dimensions: usize,
    ) -> Result<Vec<Vec<f32>>, TransportError> {
        if expected_dimensions == 0
            || config
                .parameters()
                .get("dimensions")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|d| d != expected_dimensions as u64)
        {
            return Err(TransportError::Config);
        }
        let key = std::env::var("OPENAI_API_KEY").ok();
        let prepared = prepare(config, texts, key.as_deref())?;
        let (url, body, authorization) = prepared.parts();
        let mut request = self.agent.post(url);
        if let Some(authorization) = authorization {
            request = request.header("Authorization", authorization);
        }
        let mut response = request.send_json(body).map_err(classify)?;
        if response.status().as_u16() != 200 {
            return Err(TransportError::Http(response.status().as_u16()));
        }
        let body = response
            .body_mut()
            .with_config()
            .read_to_vec()
            .map_err(classify)?;
        decode(config.provider, &body, texts.len(), expected_dimensions)
    }

    /// Separate test probe: public endpoint, no credential, input or inference.
    pub fn system_tls_probe(&self) -> Result<(), TransportError> {
        let response = self
            .agent
            .get("https://example.com/")
            .call()
            .map_err(classify)?;
        if response.status().as_u16() != 200 {
            return Err(TransportError::Http(response.status().as_u16()));
        }
        Ok(())
    }
}

fn classify(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::Timeout(_) => TransportError::Timeout,
        ureq::Error::Io(ref io) if io.kind() == std::io::ErrorKind::TimedOut => {
            TransportError::Timeout
        }
        _ => TransportError::Connection,
    }
}
