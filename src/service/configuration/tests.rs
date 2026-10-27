use super::ConfigurationService;
use crate::service::{
    CommandErrorKind, CommandResult, ConfigReader, EmbeddingConfig, EmbeddingError,
    EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult, HeldLock, ProviderKind, StatusFiles,
};
use serde_json::json;
use std::{cell::Cell, error::Error, io, num::NonZeroUsize};

const CONFIG: &[u8] = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}"#;

#[test]
fn reads_missing_config_without_creating_service_state() {
    // Given
    let files = Files {
        config: None,
        fail: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };

    // When
    let config = service.read_config().unwrap();

    // Then
    assert!(config.is_none());
    assert_eq!(files.reads.get(), 1);
    assert_eq!(files.lock_observations.get(), 0);
    assert_eq!(provider.validations.get(), 0);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn returns_preserved_config_values() {
    // Given
    let files = Files {
        config: Some(CONFIG.to_vec()),
        fail: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };

    // When
    let config = service.read_config().unwrap().unwrap();

    // Then
    assert_eq!(config.get_provider(), ProviderKind::Ollama);
    assert_eq!(config.get_model(), "fixture");
    assert_eq!(
        serde_json::Value::Object(config.get_parameters().clone()),
        json!({
            "base_url": "http://127.0.0.1:9",
            "custom": {"enabled": true},
            "dimensions": 3
        })
    );
    assert_eq!(config.get_dimensions(), NonZeroUsize::new(3));
    assert_eq!(config.get_max_attempts(), 3);
    assert_eq!(files.reads.get(), 1);
    assert_eq!(files.lock_observations.get(), 0);
    assert_eq!(provider.validations.get(), 1);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn rejects_invalid_config_without_exposing_bytes() {
    // Given
    let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","api_key":"CONFIG_CANARY"},"max_retries":0}"#;
    let files = Files {
        config: Some(bytes.to_vec()),
        fail: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };

    // When
    let error = service.read_config().unwrap_err();

    // Then
    assert_invalid_configuration_error(&error);
    assert_eq!(files.reads.get(), 1);
    assert_eq!(files.lock_observations.get(), 0);
    assert_eq!(provider.validations.get(), 0);
    assert_eq!(provider.embeddings.get(), 0);

    // Given: a syntactically valid configuration rejected by local provider validation.
    let files = Files {
        config: Some(CONFIG.to_vec()),
        fail: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::rejecting();
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };

    // When
    let error = service.read_config().unwrap_err();

    // Then
    assert_invalid_configuration_error(&error);
    assert_eq!(provider.validations.get(), 1);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn maps_read_failure_without_exposing_causes() {
    // Given
    let files = Files {
        config: None,
        fail: true,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };

    // When
    let error = service.read_config().unwrap_err();

    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.get_message(), "cannot read embedding configuration");
    assert!(!format!("{error:?}").contains("CONFIG_CANARY"));
    assert!(!error.to_string().contains("CONFIG_CANARY"));
    assert!(
        !error
            .source()
            .unwrap()
            .to_string()
            .contains("CONFIG_CANARY")
    );
    assert_eq!(provider.validations.get(), 0);
    assert_eq!(provider.embeddings.get(), 0);
}

fn assert_invalid_configuration_error(error: &crate::service::CommandError) {
    assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
    assert_eq!(error.get_message(), "invalid embedding configuration");
    assert!(!format!("{error:?}").contains("CONFIG_CANARY"));
    assert!(!error.to_string().contains("CONFIG_CANARY"));
    assert!(
        !error
            .source()
            .unwrap()
            .to_string()
            .contains("CONFIG_CANARY")
    );
}

struct Files {
    config: Option<Vec<u8>>,
    fail: bool,
    reads: Cell<usize>,
    lock_observations: Cell<usize>,
}

impl StatusFiles for Files {
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        self.reads.set(self.reads.get() + 1);
        if self.fail {
            return Err(EmbeddingError::new_with_cause(
                EmbeddingErrorKind::InternalError,
                io::Error::other("CONFIG_CANARY"),
            ));
        }
        Ok(self.config.clone())
    }

    fn observe_locks(&self) -> CommandResult<Vec<HeldLock>> {
        self.lock_observations.set(self.lock_observations.get() + 1);
        Ok(Vec::new())
    }
}

struct Provider {
    reject: bool,
    validations: Cell<usize>,
    embeddings: Cell<usize>,
}

impl Provider {
    fn accepting() -> Self {
        Self {
            reject: false,
            validations: Cell::new(0),
            embeddings: Cell::new(0),
        }
    }

    fn rejecting() -> Self {
        Self {
            reject: true,
            validations: Cell::new(0),
            embeddings: Cell::new(0),
        }
    }
}

impl EmbeddingProvider for Provider {
    fn validate_config(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
        self.validations.set(self.validations.get() + 1);
        if self.reject {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
        }
        Ok(())
    }

    fn embed(
        &self,
        _: &EmbeddingConfig,
        _: &[&str],
        _: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.embeddings.set(self.embeddings.get() + 1);
        Ok(Vec::new())
    }
}
