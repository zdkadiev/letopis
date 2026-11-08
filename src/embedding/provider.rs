use crate::service::{
    EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult,
    MAX_DIMENSIONS, MIN_DIMENSIONS, ProviderKind,
};
use crate::utils::JsonValue;
use serde_json::{Value, json};
use std::{fmt, io::ErrorKind, num::NonZeroUsize, time::Duration};
use ureq::{
    Agent,
    http::Uri,
    tls::{RootCerts, TlsConfig},
};

const ATTEMPT_DEADLINE: Duration = Duration::from_secs(60);
// Match ureq's ordinary read_to_vec cap explicitly for the configured reader.
const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

struct Http {
    agent: Agent,
}

/// Synchronous native and compatible protocols with independently owned HTTP pools.
pub(crate) struct HttpProviders {
    ollama_http: Http,
    openai_http: Http,
    key: Option<String>,
}

impl Http {
    fn new() -> Self {
        Self::new_with_deadline(ATTEMPT_DEADLINE)
    }

    /// Sends one JSON POST without redirects or proxies under the agent's shared deadline.
    ///
    /// # Arguments
    ///
    /// * `endpoint` — the complete HTTP(S) URL that receives the JSON POST.
    /// * `body` — the JSON request body serialized into the POST.
    /// * `authorization` — the optional Authorization header value; when absent, no header is
    ///   sent.
    ///
    /// # Errors
    /// Reports transport failures, non-200 statuses, oversized bodies or malformed JSON responses.
    fn request_send_by_endpoint(
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
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(classify)?;
        let JsonValue(value) =
            serde_json::from_slice::<JsonValue<false>>(&bytes).map_err(|cause| {
                EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidResponse, cause)
            })?;
        if value.get("error").is_some() {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        Ok(value)
    }

    fn new_with_deadline(deadline: Duration) -> Self {
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
}

impl HttpProviders {
    /// Creates the provider with captured credentials and one HTTP agent per protocol.
    ///
    /// # Arguments
    ///
    /// * `key` — the optional compatible API key captured once for `OpenAI` requests.
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
                ollama_vectors_compute(&self.ollama_http, config, texts, dimensions)
            }
            ProviderKind::Openai => openai_vectors_compute(
                &self.openai_http,
                self.key.as_deref(),
                config,
                texts,
                dimensions,
            ),
        }
    }
}

/// Validates the selected protocol and complete HTTP(S) endpoint without network I/O.
///
/// # Arguments
///
/// * `config` — the embedding configuration whose selected provider and `base_url` are validated.
/// * `provider` — the provider required by the caller; the configuration must select this same
///   provider.
///
/// # Errors
/// Reports unsupported protocols or unsafe, incomplete or malformed endpoints.
fn config_validate(config: &EmbeddingConfig, provider: ProviderKind) -> EmbeddingResult<()> {
    if config.get_provider() != provider {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    let base = config
        .get_parameters()
        .get("base_url")
        .and_then(Value::as_str)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let base = base.split('#').next().unwrap_or_default();
    let uri: Uri = base.parse().map_err(|cause| {
        EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
    })?;
    let authority = uri
        .authority()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    if !matches!(scheme, "http" | "https")
        || authority.as_str().contains('@')
        || authority.host().is_empty()
        || (authority.as_str() != authority.host() && authority.port_u16().is_none())
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(())
}

/// Computes native vectors without credentials, requiring untruncated input and a model response.
///
/// # Arguments
///
/// * `http` — the HTTP client used to POST the native embeddings request.
/// * `config` — the embedding configuration supplying the endpoint, model and parameters;
///   it must select the Ollama provider.
/// * `texts` — the input texts; they must be nonempty and are sent untruncated.
/// * `dimensions` — the optional requested vector size; it must match the configured
///   dimensions when both are set.
///
/// # Errors
/// Reports invalid configuration/input, transport failure or malformed native vectors.
fn ollama_vectors_compute(
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
        dimensions.or(config.get_dimensions_by_parameters()),
        ProviderKind::Ollama,
    )
}

/// Computes compatible vectors with validated credentials and restores their input order.
///
/// # Arguments
///
/// * `http` — the HTTP client used to POST the compatible embeddings request.
/// * `key` — the optional API key; it must be nonempty printable ASCII without surrounding
///   spaces to form the bearer credential.
/// * `config` — the embedding configuration supplying the endpoint, model and parameters;
///   it must select the `OpenAI` provider.
/// * `texts` — the input texts; they must be nonempty and are restored to this order in
///   the result.
/// * `dimensions` — the optional requested vector size; it must match the configured
///   dimensions when both are set.
///
/// # Errors
/// Reports invalid configuration/input, credentials, transport failure or malformed indexed
/// vectors.
fn openai_vectors_compute(
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
        dimensions.or(config.get_dimensions_by_parameters()),
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
#[path = "provider_test.rs"]
mod tests;

fn build_endpoint(
    config: &EmbeddingConfig,
    provider: ProviderKind,
    suffix: &str,
) -> EmbeddingResult<String> {
    config_validate(config, provider)?;
    let base = config
        .get_parameters()
        .get("base_url")
        .and_then(Value::as_str)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let base = base.split('#').next().unwrap_or_default();
    let uri: Uri = base.parse().map_err(|cause| {
        EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
    })?;
    let authority = uri
        .authority()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let prefix = uri.path().trim_end_matches('/');
    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Ok(format!("{scheme}://{authority}{prefix}/{suffix}{query}"))
}

fn prepare_body(
    config: &EmbeddingConfig,
    texts: &[&str],
    dimensions: Option<NonZeroUsize>,
) -> EmbeddingResult<Value> {
    if texts.is_empty()
        || texts.iter().any(|text| text.is_empty())
        || dimensions.is_some_and(|size| !(MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(&size.get()))
        || dimensions
            .zip(config.get_dimensions_by_parameters())
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

fn decode_vectors(
    items: &[Value],
    count: usize,
    dimensions: Option<NonZeroUsize>,
    provider: ProviderKind,
) -> EmbeddingResult<Vec<Vec<f32>>> {
    if items.len() != count {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
    }
    let mut result = vec![None; count];
    let mut expected = dimensions;
    for (position, item) in items.iter().enumerate() {
        let (index, vector) = get_index_vector(position, item, count, provider)?;
        let vector = get_vector(vector, expected)?;
        expected = NonZeroUsize::new(vector.len());
        if result[index].replace(vector).is_some() {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
    }
    result
        .into_iter()
        .map(|vector| {
            vector.ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))
        })
        .collect()
}

fn get_index_vector(
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
                .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
            Ok((
                index,
                item.get("embedding")
                    .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?,
            ))
        }
    }
}

fn get_vector(embedding: &Value, expected: Option<NonZeroUsize>) -> EmbeddingResult<Vec<f32>> {
    let values = embedding
        .as_array()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
    if !(MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(&values.len())
        || expected.is_some_and(|size| values.len() != size.get())
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
    }
    values.iter().map(get_component).collect()
}

fn get_component(component_value: &Value) -> EmbeddingResult<f32> {
    let number = component_value
        .as_f64()
        .filter(|number| number.is_finite() && number.abs() <= f64::from(f32::MAX))
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidResponse))?;
    // Conversion is deliberately rounded to float32 after checking its finite range.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "range was checked and float32 rounding is intended"
    )]
    let component = number as f32;
    Ok(component)
}

fn classify(error: ureq::Error) -> EmbeddingError {
    let kind = match &error {
        ureq::Error::BodyExceedsLimit(_) => EmbeddingErrorKind::InvalidResponse,
        ureq::Error::Timeout(_) => EmbeddingErrorKind::Timeout,
        ureq::Error::Io(error) if error.kind() == ErrorKind::TimedOut => {
            EmbeddingErrorKind::Timeout
        }
        _ => EmbeddingErrorKind::Connection,
    };
    EmbeddingError::new_with_cause(kind, error)
}
