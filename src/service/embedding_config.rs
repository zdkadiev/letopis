use super::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use crate::domain::Sha256;
use crate::utils::JsonValue;
use serde_json::{Map, Value};
use std::{fmt, num::NonZeroUsize};

/// Smallest embedding vector length a provider may return.
pub(crate) const MIN_DIMENSIONS: usize = 2;
/// Largest embedding vector length a provider may return.
pub(crate) const MAX_DIMENSIONS: usize = 4096;
const ROOT_FIELDS: [&str; 4] = ["provider", "model", "parameters", "max_retries"];

/// Supported finite protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Native Ollama embeddings.
    Ollama,
    /// OpenAI-compatible embeddings.
    Openai,
}
/// Parsed application configuration without process credentials.
#[derive(Clone)]
pub struct EmbeddingConfig {
    provider: ProviderKind,
    model: String,
    parameters: Map<String, Value>,
    identity: Sha256,
    max_attempts: u64,
    dimensions: Option<NonZeroUsize>,
}
impl EmbeddingConfig {
    /// Parse unique-key JSON and compute its RFC 8785 identity.
    /// Endpoint syntax is subsequently checked by the provider boundary.
    ///
    /// # Errors
    /// Rejects malformed shapes, forbidden keys, invalid parameters, unsupported
    /// dimensions and budgets not exactly preserved by JCS or storage integers.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let JsonValue(value) =
            serde_json::from_slice::<JsonValue<true>>(bytes).map_err(map_config_error)?;
        let root = value.as_object().ok_or_else(reject_config)?;
        if root.len() != ROOT_FIELDS.len()
            || root.keys().any(|key| !ROOT_FIELDS.contains(&key.as_str()))
        {
            return Err(reject_config());
        }
        let provider = match root.get("provider").and_then(Value::as_str) {
            Some("ollama") => ProviderKind::Ollama,
            Some("openai") => ProviderKind::Openai,
            _ => return Err(reject_config()),
        };
        let model = root
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .ok_or_else(reject_config)?;
        let parameters = root
            .get("parameters")
            .and_then(Value::as_object)
            .ok_or_else(reject_config)?;
        validate_parameters(parameters)?;
        let dimensions = read_dimensions(parameters)?;
        let retries = root
            .get("max_retries")
            .and_then(Value::as_u64)
            .ok_or_else(reject_config)?;
        let max_attempts = retries
            .checked_add(1)
            .filter(|count| i64::try_from(*count).is_ok())
            .ok_or_else(reject_config)?;
        let canonical = serde_json_canonicalizer::to_vec(&value).map_err(map_config_error)?;
        verify_execution_identity(&value, &canonical)?;
        Ok(Self {
            provider,
            model: model.to_owned(),
            parameters: parameters.clone(),
            identity: Sha256::digest(&canonical),
            max_attempts,
            dimensions,
        })
    }
    /// Return the selected protocol.
    pub fn get_provider(&self) -> ProviderKind {
        self.provider
    }
    /// Return the configured model name.
    pub fn get_model(&self) -> &str {
        &self.model
    }
    /// Return allowed configuration values, including preserved unknown parameters.
    pub fn get_parameters(&self) -> &Map<String, Value> {
        &self.parameters
    }
    /// Return the hash of the complete canonical configuration.
    pub fn get_identity(&self) -> Sha256 {
        self.identity
    }
    /// Return the checked total attempt budget, including the initial attempt.
    pub fn get_max_attempts(&self) -> u64 {
        self.max_attempts
    }
    /// Return explicitly requested native dimensions.
    pub fn get_dimensions(&self) -> Option<NonZeroUsize> {
        self.dimensions
    }
}
impl fmt::Debug for EmbeddingConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddingConfig")
            .field("provider", &self.provider)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
fn reject_config() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)
}
fn map_config_error(cause: impl std::error::Error + Send + Sync + 'static) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
}
fn validate_parameters(parameters: &Map<String, Value>) -> EmbeddingResult<()> {
    if parameters
        .get("base_url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .is_none()
        || parameters.keys().any(|key| {
            key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("api_key")
        })
        || parameters.get("keep_alive").is_some_and(|v| !v.is_string())
        || parameters.get("options").is_some_and(|v| !v.is_object())
        || parameters
            .get("truncate")
            .is_some_and(|v| v.as_bool() != Some(false))
        || parameters
            .get("encoding_format")
            .is_some_and(|v| v.as_str() != Some("float"))
    {
        return Err(reject_config());
    }
    Ok(())
}
fn read_dimensions(parameters: &Map<String, Value>) -> EmbeddingResult<Option<NonZeroUsize>> {
    parameters
        .get("dimensions")
        .map(|value| {
            let size = value
                .as_u64()
                .and_then(|size| usize::try_from(size).ok())
                .filter(|size| (MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(size))
                .ok_or_else(reject_config)?;
            NonZeroUsize::new(size).ok_or_else(reject_config)
        })
        .transpose()
}
fn verify_execution_identity(original: &Value, bytes: &[u8]) -> EmbeddingResult<()> {
    let JsonValue(canonical) =
        serde_json::from_slice::<JsonValue<false>>(bytes).map_err(map_config_error)?;
    if !has_exact_integers(original, &canonical) {
        return Err(reject_config());
    }
    Ok(())
}
fn has_exact_integers(original: &Value, canonical: &Value) -> bool {
    match original {
        Value::Number(number) if number.is_u64() => number.as_u64() == canonical.as_u64(),
        Value::Number(number) if number.is_i64() => number.as_i64() == canonical.as_i64(),
        Value::Array(values) => canonical.as_array().is_some_and(|other| {
            values.len() == other.len()
                && values
                    .iter()
                    .zip(other)
                    .all(|(a, b)| has_exact_integers(a, b))
        }),
        Value::Object(values) => canonical.as_object().is_some_and(|other| {
            values.iter().all(|(key, value)| {
                other
                    .get(key)
                    .is_some_and(|other| has_exact_integers(value, other))
            })
        }),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::EmbeddingConfig;
    use crate::utils::JsonValue;

    #[test]
    fn bounds_raw_token_traversal_before_canonicalization() {
        // Arrange
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        // Act
        let result = serde_json::from_str::<JsonValue<true>>(&nested);
        // Assert
        assert!(result.is_err());
    }
    #[test]
    fn rejects_trailing_json_after_configuration() {
        // Arrange
        let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0} null"#;
        // Act
        let result = EmbeddingConfig::parse(bytes);
        // Assert
        assert!(result.is_err());
    }
}
