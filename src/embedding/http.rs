use crate::service::{
    EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingResult, ProviderKind,
};
use serde_json::{Value, json};
use std::{num::NonZeroUsize, time::Duration};
use ureq::{
    Agent,
    http::Uri,
    tls::{RootCerts, TlsConfig},
};

const ATTEMPT_DEADLINE: Duration = Duration::from_secs(60);
const MIN_DIMENSIONS: usize = 2;
const MAX_DIMENSIONS: usize = 4096;

pub(super) struct Http {
    agent: Agent,
}
impl Http {
    pub(super) fn new() -> Self {
        Self::with_deadline(ATTEMPT_DEADLINE)
    }
    fn with_deadline(deadline: Duration) -> Self {
        let agent = Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .timeout_global(Some(deadline))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .new_agent();
        Self { agent }
    }
    pub(super) fn post(
        &self,
        endpoint: &str,
        body: &Value,
        authorization: Option<&str>,
    ) -> EmbeddingResult<Value> {
        let mut request = self.agent.post(endpoint);
        if let Some(authorization) = authorization {
            request = request.header("Authorization", authorization);
        }
        let mut response = request.send_json(body).map_err(classify)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(EmbeddingError::new(EmbeddingErrorKind::HttpStatus(status)));
        }
        let bytes = response
            .body_mut()
            .with_config()
            .read_to_vec()
            .map_err(classify)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|cause| {
            EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidResponse, cause)
        })?;
        if value.get("error").is_some() {
            return Err(invalid_response());
        }
        Ok(value)
    }
}
pub(super) fn build_endpoint(
    config: &EmbeddingConfig,
    provider: ProviderKind,
    suffix: &str,
) -> EmbeddingResult<String> {
    let invalid = || EmbeddingError::new(EmbeddingErrorKind::InvalidConfig);
    if config.get_provider() != provider {
        return Err(invalid());
    }
    let base = config
        .get_parameters()
        .get("base_url")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let base = base.split('#').next().unwrap_or_default();
    let uri: Uri = base.parse().map_err(|cause| {
        EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
    })?;
    let authority = uri.authority().ok_or_else(invalid)?;
    let scheme = uri.scheme_str().ok_or_else(invalid)?;
    if !matches!(scheme, "http" | "https")
        || authority.as_str().contains('@')
        || authority.host().is_empty()
        || (authority.as_str() != authority.host() && authority.port_u16().is_none())
    {
        return Err(invalid());
    }
    let prefix = uri.path().trim_end_matches('/');
    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Ok(format!("{scheme}://{authority}{prefix}/{suffix}{query}"))
}
pub(super) fn prepare_body(
    config: &EmbeddingConfig,
    texts: &[&str],
    dimensions: Option<NonZeroUsize>,
) -> EmbeddingResult<Value> {
    if texts.is_empty()
        || texts.iter().any(|text| text.is_empty())
        || dimensions.is_some_and(|size| !(MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(&size.get()))
        || dimensions
            .zip(config.get_dimensions())
            .is_some_and(|(expected, requested)| expected != requested)
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::Input));
    }
    let mut body = json!({"model":config.get_model(),"input":texts});
    if let Some(size) = config.get_parameters().get("dimensions") {
        body["dimensions"] = size.clone();
    }
    Ok(body)
}
pub(super) fn decode_vectors(
    items: &[Value],
    count: usize,
    dimensions: Option<NonZeroUsize>,
    provider: ProviderKind,
) -> EmbeddingResult<Vec<Vec<f32>>> {
    if items.len() != count {
        return Err(invalid_response());
    }
    let mut result = vec![None; count];
    let mut expected = dimensions;
    for (position, item) in items.iter().enumerate() {
        let (index, vector) = read_index_vector(position, item, count, provider)?;
        let vector = read_vector(vector, expected)?;
        expected = NonZeroUsize::new(vector.len());
        if result[index].replace(vector).is_some() {
            return Err(invalid_response());
        }
    }
    result
        .into_iter()
        .map(|vector| vector.ok_or_else(invalid_response))
        .collect()
}
fn read_index_vector(
    position: usize,
    item: &Value,
    count: usize,
    provider: ProviderKind,
) -> EmbeddingResult<(usize, &Value)> {
    match provider {
        ProviderKind::Ollama => Ok((position, item)),
        ProviderKind::Openai => {
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < count)
                .ok_or_else(invalid_response)?;
            Ok((index, item.get("embedding").ok_or_else(invalid_response)?))
        }
    }
}
fn read_vector(value: &Value, expected: Option<NonZeroUsize>) -> EmbeddingResult<Vec<f32>> {
    let values = value.as_array().ok_or_else(invalid_response)?;
    if !(MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(&values.len())
        || expected.is_some_and(|size| values.len() != size.get())
    {
        return Err(invalid_response());
    }
    values.iter().map(read_component).collect()
}
fn read_component(value: &Value) -> EmbeddingResult<f32> {
    let number = value
        .as_f64()
        .filter(|number| number.is_finite() && number.abs() <= f64::from(f32::MAX))
        .ok_or_else(invalid_response)?;
    // Conversion is deliberately rounded to float32 after checking its finite range.
    #[allow(clippy::cast_possible_truncation)]
    let component = number as f32;
    Ok(component)
}
pub(super) fn invalid_response() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::InvalidResponse)
}
fn classify(error: ureq::Error) -> EmbeddingError {
    let kind = match &error {
        ureq::Error::Timeout(_) => EmbeddingErrorKind::Timeout,
        ureq::Error::Io(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            EmbeddingErrorKind::Timeout
        }
        _ => EmbeddingErrorKind::Connection,
    };
    EmbeddingError::new_with_cause(kind, error)
}

#[cfg(test)]
mod tests {
    use super::Http;
    use crate::service::EmbeddingErrorKind;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    fn run_deadline_case(header_delay: Duration, body_delay: Duration) {
        // Arrange
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/embed", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut received = Vec::new();
            loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                received.push(byte[0]);
                if received.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            thread::sleep(header_delay);
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n");
            thread::sleep(body_delay);
            let _ = socket.write_all(b"{}");
        });
        let http = Http::with_deadline(Duration::from_millis(120));
        let start = Instant::now();
        // Act
        let result = http.post(&endpoint, &json!({"input":["fixture"]}), None);
        let elapsed = start.elapsed();
        worker.join().unwrap();
        // Assert
        assert_eq!(result.unwrap_err().get_kind(), EmbeddingErrorKind::Timeout);
        assert!(elapsed < Duration::from_secs(2));
    }
    #[test]
    fn bounds_waiting_for_response_headers() {
        run_deadline_case(Duration::from_millis(240), Duration::ZERO);
    }
    #[test]
    fn bounds_waiting_for_response_body() {
        run_deadline_case(Duration::ZERO, Duration::from_millis(240));
    }
    #[test]
    fn shares_one_deadline_across_headers_and_body() {
        run_deadline_case(Duration::from_millis(80), Duration::from_millis(80));
    }
}
