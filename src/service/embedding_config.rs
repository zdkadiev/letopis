use super::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use crate::domain::Sha256;
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, value::RawValue};
use std::{fmt, num::NonZeroUsize};

// Preserve serde_json's default nesting budget across borrowed-token parsers.
const MAX_JSON_DEPTH: usize = 128;
const MIN_DIMENSIONS: usize = 2;
const MAX_DIMENSIONS: usize = 4096;
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
        let UniqueValue(value) = serde_json::from_slice(bytes).map_err(invalid_cause)?;
        let root = value.as_object().ok_or_else(invalid)?;
        if root.len() != ROOT_FIELDS.len()
            || root.keys().any(|key| !ROOT_FIELDS.contains(&key.as_str()))
        {
            return Err(invalid());
        }
        let provider = match root.get("provider").and_then(Value::as_str) {
            Some("ollama") => ProviderKind::Ollama,
            Some("openai") => ProviderKind::Openai,
            _ => return Err(invalid()),
        };
        let model = root
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .ok_or_else(invalid)?;
        let parameters = root
            .get("parameters")
            .and_then(Value::as_object)
            .ok_or_else(invalid)?;
        validate_parameters(parameters)?;
        let dimensions = read_dimensions(parameters)?;
        let retries = root
            .get("max_retries")
            .and_then(Value::as_u64)
            .ok_or_else(invalid)?;
        let max_attempts = retries
            .checked_add(1)
            .filter(|count| i64::try_from(*count).is_ok())
            .ok_or_else(invalid)?;
        let canonical = serde_json_canonicalizer::to_vec(&value).map_err(invalid_cause)?;
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
fn invalid() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)
}
fn invalid_cause(cause: impl std::error::Error + Send + Sync + 'static) -> EmbeddingError {
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
        return Err(invalid());
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
                .ok_or_else(invalid)?;
            NonZeroUsize::new(size).ok_or_else(invalid)
        })
        .transpose()
}
fn verify_execution_identity(original: &Value, bytes: &[u8]) -> EmbeddingResult<()> {
    let canonical: Value = serde_json::from_slice(bytes).map_err(invalid_cause)?;
    if !has_exact_integers(original, &canonical) {
        return Err(invalid());
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

// Deserialize recursively before constructing any Value object, so duplicates
// cannot disappear into serde_json's last-key-wins representation.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        UniqueValueSeed { depth: 0 }.deserialize(deserializer)
    }
}
struct UniqueValueSeed {
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for UniqueValueSeed {
    type Value = UniqueValue;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if self.depth >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON nesting exceeds supported limit"));
        }
        let raw = <&RawValue>::deserialize(deserializer)?;
        if !has_supported_integer_token(raw.get()) {
            return Err(de::Error::custom("integer outside supported range"));
        }
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        parser
            .deserialize_any(UniqueVisitor { depth: self.depth })
            .map_err(de::Error::custom)
    }
}
// RawValue borrows the original token; serde_json would otherwise turn an
// overflowing integer into f64 before the visitor can check its exact value.
fn has_supported_integer_token(raw: &str) -> bool {
    let is_number = raw
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit() || *first == b'-');
    if !is_number || raw.contains(['.', 'e', 'E']) {
        return true;
    }
    if raw.starts_with('-') {
        raw.parse::<i64>().is_ok()
    } else {
        raw.parse::<u64>().is_ok()
    }
}
struct UniqueVisitor {
    depth: usize,
}
impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unique-key JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(v.into()))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(v)
            .map(|n| UniqueValue(Value::Number(n)))
            .ok_or_else(|| E::custom("invalid number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue(v.into()))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(UniqueValue(value)) = sequence.next_element_seed(UniqueValueSeed {
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(UniqueValue(Value::Array(values)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            let UniqueValue(value) = object.next_value_seed(UniqueValueSeed {
                depth: self.depth + 1,
            })?;
            if values.insert(key, value).is_some() {
                return Err(de::Error::custom("duplicate key"));
            }
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueValue;

    #[test]
    fn bounds_raw_token_traversal_before_canonicalization() {
        // Arrange
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        // Act
        let result = serde_json::from_str::<UniqueValue>(&nested);
        // Assert
        assert!(result.is_err());
    }
}
