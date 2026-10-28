use super::{EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use crate::utils::JsonValue;
use serde_json::{Map, Value};

const ROOT_SECTION: &str = "embedding_model";
const SECTION_FIELDS: [&str; 5] = ["enabled", "provider", "model", "parameters", "max_retries"];
const LEGACY_FIELDS: [&str; 4] = ["provider", "model", "parameters", "max_retries"];

/// Complete generated configuration written by initialization.
pub(crate) const GENERATED_CONFIG: &[u8] = b"{\n  \"embedding_model\": {\n    \"enabled\": false,\n    \"provider\": null,\n    \"model\": null,\n    \"parameters\": {},\n    \"max_retries\": 0\n  }\n}\n";

/// Validated complete `.letopis/config.json` value.
#[derive(Debug, Clone)]
pub enum Configuration {
    /// Embeddings are off and no provider settings are retained.
    Unconfigured {
        /// Additional configured retries, preserved for later setup.
        max_retries: u64,
    },
    /// Locally valid provider settings are retained with execution disabled.
    Disabled(EmbeddingConfig),
    /// Locally valid provider settings with embedding execution enabled.
    Enabled(EmbeddingConfig),
}

impl Configuration {
    /// Parse one strict nested configuration document.
    ///
    /// The root contains exactly the `embedding_model` section. Configured
    /// provider settings use the same four-field projection and JCS identity as
    /// [`EmbeddingConfig`], excluding the section, `enabled` and root wrapper.
    ///
    /// # Errors
    /// Rejects legacy flat documents, malformed or duplicate-key JSON, unknown or
    /// missing fields, partial provider/model settings, inconsistent unconfigured
    /// values, invalid retry counts and configured settings rejected locally.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let JsonValue(value) =
            serde_json::from_slice::<JsonValue<true>>(bytes).map_err(map_config_error)?;
        let root = value.as_object().ok_or_else(reject_config)?;
        if root.len() == 1
            && let Some(section) = root.get(ROOT_SECTION)
        {
            return parse_section(section);
        }
        if root.keys().any(|key| LEGACY_FIELDS.contains(&key.as_str())) {
            return Err(EmbeddingError::new(EmbeddingErrorKind::MigrationRequired));
        }
        Err(reject_config())
    }

    /// Return whether configured embedding execution is enabled.
    pub fn get_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    /// Return the configured retry count, including the unconfigured document.
    pub fn get_max_retries(&self) -> u64 {
        match self {
            Self::Unconfigured { max_retries } => *max_retries,
            Self::Disabled(config) | Self::Enabled(config) => {
                config.get_max_attempts().saturating_sub(1)
            }
        }
    }

    /// Return validated provider settings when the document retains any.
    pub fn get_embedding_config(&self) -> Option<&EmbeddingConfig> {
        match self {
            Self::Unconfigured { .. } => None,
            Self::Disabled(config) | Self::Enabled(config) => Some(config),
        }
    }
}

fn parse_section(value: &Value) -> EmbeddingResult<Configuration> {
    let section = value.as_object().ok_or_else(reject_config)?;
    if section.len() != SECTION_FIELDS.len()
        || section
            .keys()
            .any(|key| !SECTION_FIELDS.contains(&key.as_str()))
    {
        return Err(reject_config());
    }
    let enabled = section
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(reject_config)?;
    let max_retries = section
        .get("max_retries")
        .and_then(Value::as_u64)
        .ok_or_else(reject_config)?;
    validate_retries(max_retries)?;
    let provider = read_optional_string(section, "provider")?;
    let model = read_optional_string(section, "model")?;
    let parameters = section
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or_else(reject_config)?;
    match (provider, model) {
        (None, None) => parse_unconfigured(enabled, parameters, max_retries),
        (Some(provider), Some(model)) => {
            parse_configured(enabled, provider, model, parameters, max_retries)
        }
        _ => Err(reject_config()),
    }
}

fn read_optional_string<'a>(
    section: &'a Map<String, Value>,
    field: &str,
) -> EmbeddingResult<Option<&'a str>> {
    match section.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(reject_config()),
    }
}

fn parse_unconfigured(
    enabled: bool,
    parameters: &Map<String, Value>,
    max_retries: u64,
) -> EmbeddingResult<Configuration> {
    if enabled || !parameters.is_empty() {
        return Err(reject_config());
    }
    Ok(Configuration::Unconfigured { max_retries })
}

fn parse_configured(
    enabled: bool,
    provider: &str,
    model: &str,
    parameters: &Map<String, Value>,
    max_retries: u64,
) -> EmbeddingResult<Configuration> {
    let projection = Value::Object(Map::from_iter([
        ("provider".into(), Value::String(provider.into())),
        ("model".into(), Value::String(model.into())),
        ("parameters".into(), Value::Object(parameters.clone())),
        ("max_retries".into(), Value::from(max_retries)),
    ]));
    let bytes = serde_json::to_vec(&projection).map_err(map_config_error)?;
    let config = EmbeddingConfig::parse(&bytes)?;
    Ok(if enabled {
        Configuration::Enabled(config)
    } else {
        Configuration::Disabled(config)
    })
}

fn validate_retries(retries: u64) -> EmbeddingResult<()> {
    retries
        .checked_add(1)
        .filter(|attempts| i64::try_from(*attempts).is_ok())
        .map(|_| ())
        .ok_or_else(reject_config)
}

fn map_config_error(cause: impl std::error::Error + Send + Sync + 'static) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
}

fn reject_config() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::{Configuration, GENERATED_CONFIG};
    use crate::{
        domain::Sha256,
        service::configuration::validate_configuration,
        service::{
            EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult,
        },
    };
    use serde::Deserialize;
    use std::num::NonZeroUsize;

    const CORPUS: &str = include_str!("../../tests/fixtures/config/corpus.json");
    const ENABLED: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;
    const DISABLED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;

    #[derive(Deserialize)]
    struct Corpus {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        name: String,
        raw: String,
        parse_valid: bool,
        validate_valid: bool,
    }
    struct Provider;

    #[test]
    fn parses_generated_defaults_and_preserves_retry_setting() {
        // Given / When
        let configuration = Configuration::parse(GENERATED_CONFIG).unwrap();

        // Then
        let Configuration::Unconfigured { max_retries } = configuration else {
            panic!("generated defaults must be unconfigured")
        };
        assert_eq!(max_retries, 0);
    }

    #[test]
    fn identity_excludes_enabled_and_the_root_wrapper() {
        // Given: the same configured projection under both execution states.
        let enabled = Configuration::parse(ENABLED).unwrap();
        let disabled = Configuration::parse(DISABLED).unwrap();
        let expected = Sha256::digest(
            br#"{"max_retries":2,"model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"provider":"ollama"}"#,
        );

        // When / Then: only the four configured settings participate.
        assert_eq!(
            enabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert_eq!(
            disabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert!(enabled.get_enabled());
        assert!(!disabled.get_enabled());
    }

    #[test]
    fn reports_legacy_flat_document_for_migration() {
        // Given
        let raw = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0}"#;

        // When
        let error = Configuration::parse(raw).unwrap_err();

        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::MigrationRequired);
    }

    #[test]
    fn shared_corpus_agrees_with_local_parse_and_validation() {
        // Given: the same corpus the schema checker consumes.
        let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();

        // When / Then: structural parsing and provider-local validation stay separate.
        for case in corpus.cases {
            let parsed = Configuration::parse(case.raw.as_bytes());
            assert_eq!(parsed.is_ok(), case.parse_valid, "{}", case.name);
            if parsed.is_ok() {
                let validated = validate_configuration(case.raw.as_bytes(), &Provider);
                assert_eq!(validated.is_ok(), case.validate_valid, "{}", case.name);
            }
        }
    }

    impl EmbeddingProvider for Provider {
        fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
            let url = config
                .get_parameters()
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                Ok(())
            } else {
                Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
            }
        }

        fn embed(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            Err(EmbeddingError::new(EmbeddingErrorKind::Input))
        }
    }
}
