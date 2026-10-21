//! Pure X-2 boundary rules. Provider payloads never enter error formatting.
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::fmt;
use ureq::http::Uri;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Ollama,
    Openai,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: Provider,
    model: String,
    parameters: Map<String, Value>,
    pub max_retries: u64,
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Self, TransportError> {
        let config: Self = serde_json::from_slice(bytes).map_err(|_| TransportError::Config)?;
        if config.model.is_empty()
            || ["api_key", "authorization", "Authorization"]
                .iter()
                .any(|key| config.parameters.contains_key(*key))
        {
            return Err(TransportError::Config);
        }
        config.endpoint()?;
        Ok(config)
    }

    pub fn parameters(&self) -> &Map<String, Value> {
        &self.parameters
    }

    fn endpoint(&self) -> Result<String, TransportError> {
        let base = self
            .parameters
            .get("base_url")
            .and_then(Value::as_str)
            .ok_or(TransportError::Config)?;
        let uri: Uri = base
            .split('#')
            .next()
            .unwrap_or_default()
            .parse()
            .map_err(|_| TransportError::Config)?;
        let authority = uri.authority().ok_or(TransportError::Config)?;
        let scheme = uri.scheme_str().ok_or(TransportError::Config)?;
        if !matches!(scheme, "http" | "https")
            || authority.as_str().contains('@')
            || authority.host().is_empty()
        {
            return Err(TransportError::Config);
        }
        let suffix = match self.provider {
            Provider::Ollama => "embed",
            Provider::Openai => "embeddings",
        };
        let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
        let prefix = uri.path().trim_end_matches('/');
        Ok(format!("{scheme}://{authority}{prefix}/{suffix}{query}"))
    }
}

pub struct PreparedRequest {
    url: String,
    body: Value,
    authorization: Option<String>,
}

impl PreparedRequest {
    /// HTTP adapter details; not part of the application port.
    pub fn parts(&self) -> (&str, &Value, Option<&str>) {
        (&self.url, &self.body, self.authorization.as_deref())
    }
}

impl fmt::Debug for PreparedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedRequest(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Config,
    Credentials,
    Response,
    Http(u16),
    Timeout,
    Connection,
    Cancelled,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(status) => write!(f, "embedding_failed: HTTP {status}"),
            Self::Config => f.write_str("embedding_failed: invalid configuration"),
            Self::Credentials => f.write_str("embedding_failed: invalid credentials"),
            Self::Response => f.write_str("embedding_failed: invalid response"),
            Self::Timeout => f.write_str("embedding_failed: timeout"),
            Self::Connection => f.write_str("embedding_failed: connection failed"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}
impl std::error::Error for TransportError {}

pub fn prepare(
    config: &Config,
    texts: &[String],
    key: Option<&str>,
) -> Result<PreparedRequest, TransportError> {
    if texts.is_empty() || texts.iter().any(String::is_empty) {
        return Err(TransportError::Config);
    }
    let mut body = json!({"model":config.model,"input":texts});
    if let Some(dimensions) = config.parameters.get("dimensions") {
        if dimensions.as_u64().is_none_or(|n| n == 0) {
            return Err(TransportError::Config);
        }
        body["dimensions"] = dimensions.clone();
    }
    let authorization = match config.provider {
        Provider::Ollama => {
            body["truncate"] = json!(false);
            for (name, valid) in [
                ("keep_alive", Value::is_string as fn(&Value) -> bool),
                ("options", Value::is_object),
            ] {
                if let Some(value) = config.parameters.get(name) {
                    if !valid(value) {
                        return Err(TransportError::Config);
                    }
                    body[name] = value.clone();
                }
            }
            None
        }
        Provider::Openai => {
            body["encoding_format"] = json!("float");
            let key = key
                .filter(|k| !k.is_empty() && k.bytes().all(|b| (0x21..=0x7e).contains(&b)))
                .ok_or(TransportError::Credentials)?;
            Some(format!("Bearer {key}"))
        }
    };
    Ok(PreparedRequest {
        url: config.endpoint()?,
        body,
        authorization,
    })
}

pub fn decode(
    provider: Provider,
    body: &[u8],
    input_count: usize,
    expected_dimensions: usize,
) -> Result<Vec<Vec<f32>>, TransportError> {
    if input_count == 0 || expected_dimensions == 0 {
        return Err(TransportError::Response);
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| TransportError::Response)?;
    if value.get("error").is_some() {
        return Err(TransportError::Response);
    }
    let field = match provider {
        Provider::Ollama => "embeddings",
        Provider::Openai => "data",
    };
    if matches!(provider, Provider::Ollama)
        && value
            .get("model")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(TransportError::Response);
    }
    let items = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(TransportError::Response)?;
    if items.len() != input_count {
        return Err(TransportError::Response);
    }
    let mut result = vec![None; input_count];
    for (position, item) in items.iter().enumerate() {
        let (index, vector) = match provider {
            Provider::Ollama => (position, item),
            Provider::Openai => {
                let index = item
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n < input_count)
                    .ok_or(TransportError::Response)?;
                (
                    index,
                    item.get("embedding").ok_or(TransportError::Response)?,
                )
            }
        };
        if result[index].is_some() {
            return Err(TransportError::Response);
        }
        let vector = vector.as_array().ok_or(TransportError::Response)?;
        if vector.len() != expected_dimensions {
            return Err(TransportError::Response);
        }
        let vector = vector
            .iter()
            .map(|number| {
                let n = number.as_f64().ok_or(TransportError::Response)? as f32;
                if n.is_finite() {
                    Ok(n)
                } else {
                    Err(TransportError::Response)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        result[index] = Some(vector);
    }
    result
        .into_iter()
        .map(|v| v.ok_or(TransportError::Response))
        .collect()
}
