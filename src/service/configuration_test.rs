use crate::service::{
    CapturedFile, FileError, FileErrorKind, FileIdentity, FileReader, FileResult, FileStat,
    MockFileReader,
};

use super::ConfigurationService;
use crate::service::{
    CommandError, CommandErrorKind, ConfigReader, EmbeddingConfig, EmbeddingError,
    EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult, ProviderKind,
};
use serde_json::json;
use std::{cell::Cell, error::Error, io, num::NonZeroUsize, os::unix::fs::MetadataExt};

const CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}}"#;
const DISABLED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9"},"max_retries":2}}"#;
const UNCONFIGURED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0}}"#;
const LEGACY: &[u8] = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9"},"max_retries":0}"#;

#[test]
fn retries_a_configuration_observation_that_raced_publication() {
    // Given: the first configuration read observes a concurrent creation.
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = std::sync::Arc::clone(&attempts);
    let mut files = MockFileReader::new();
    files
        .expect_directory_get_by_path()
        .times(2)
        .returning(|_| Ok(None));
    files.expect_file_get_bytes_by_path().returning(move |_| {
        let attempt = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if attempt == 1 {
            Err(FileError::new(FileErrorKind::Changed))
        } else {
            Ok(Some(UNCONFIGURED.to_vec()))
        }
    });
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };
    // When: configuration is observed while another process publishes it.
    let config = service.get_config().unwrap();
    // Then: the retried observation returns the published configuration.
    assert!(
        config
            .as_ref()
            .is_some_and(|config| !config.is_enabled() && config.get_embedding_config().is_none())
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn reads_configuration_at_extreme_filesystem_times() {
    // Given: valid configuration files with old and future times accepted by this filesystem.
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".letopis");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("config.json");
    std::fs::write(&path, UNCONFIGURED).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let files = crate::filesystem::LocalFileSystem::new(root.path()).unwrap();
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };
    for time in [
        std::time::UNIX_EPOCH - std::time::Duration::from_secs(10_000_000_000),
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000_000_000),
    ] {
        file.set_times(std::fs::FileTimes::new().set_modified(time))
            .unwrap();
        let actual = file.metadata().unwrap();
        eprintln!(
            "filesystem mtime: {} seconds, {} nanos",
            actual.mtime(),
            actual.mtime_nsec()
        );
        // When: the configuration service reads the file.
        let config = service.get_config().unwrap();
        // Then: valid configuration remains readable regardless of its stored timestamp.
        assert!(
            config.as_ref().is_some_and(
                |config| !config.is_enabled() && config.get_embedding_config().is_none()
            )
        );
    }
}

#[test]
fn selects_literal_configuration_path_without_writes() {
    // Given: a reader accepting only the application configuration path.
    let mut files = MockFileReader::new();
    files
        .expect_directory_get_by_path()
        .withf(|path| path == std::path::Path::new(".letopis"))
        .times(1)
        .returning(|_| Ok(None));
    files
        .expect_file_get_bytes_by_path()
        .withf(|path| path == std::path::Path::new(".letopis/config.json"))
        .times(1)
        .returning(|_| Ok(None));
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };
    // When: configuration is observed on an uninitialized checkout.
    let result = service.get_config().unwrap();
    // Then: absence is preserved and provider validation is unnecessary.
    assert!(result.is_none());
    assert_eq!(provider.validations.get(), 0);
}

#[test]
fn reads_missing_config_without_creating_service_state() {
    // Given
    let files = Files {
        config: None,
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let config = service.get_config().unwrap();

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
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let configuration = service.get_config().unwrap().unwrap();
    let config = configuration.get_embedding_config().unwrap();

    // Then
    assert!(configuration.is_enabled());
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
    assert_eq!(config.get_dimensions_by_parameters(), NonZeroUsize::new(3));
    assert_eq!(config.get_max_attempts(), 3);
    assert_eq!(files.reads.get(), 1);
    assert_eq!(files.lock_observations.get(), 0);
    assert_eq!(provider.validations.get(), 1);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn accepts_unconfigured_without_provider_validation() {
    // Given: the generated unconfigured document.
    let files = Files {
        config: Some(UNCONFIGURED.to_vec()),
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::rejecting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let configuration = service.get_config().unwrap().unwrap();

    // Then: no execution identity exists and the provider boundary is untouched.
    assert!(!configuration.is_enabled());
    assert_eq!(configuration.get_max_retries(), 0);
    assert!(configuration.get_embedding_config().is_none());
    assert_eq!(provider.validations.get(), 0);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn validates_configured_disabled_settings_locally() {
    // Given: a disabled document with retained provider settings.
    let files = Files {
        config: Some(DISABLED.to_vec()),
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::rejecting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let error = service.get_config().unwrap_err();

    // Then: disabled settings still pass through local provider validation.
    assert_invalid_configuration_error(&error);
    assert_eq!(provider.validations.get(), 1);
    assert_eq!(provider.embeddings.get(), 0);
}

#[test]
fn reports_legacy_flat_configuration_as_migration() {
    // Given: an old four-field file.
    let files = Files {
        config: Some(LEGACY.to_vec()),
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let error = service.get_config().unwrap_err();

    // Then: the safe top-level message names the required migration.
    assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
    assert_eq!(
        error.get_message(),
        "embedding configuration requires migration"
    );
    assert_eq!(provider.validations.get(), 0);
}

#[test]
fn rejects_invalid_config_without_exposing_bytes() {
    // Given
    let bytes = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","api_key":"CONFIG_CANARY"},"max_retries":0}}"#;
    let files = Files {
        config: Some(bytes.to_vec()),
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let error = service.get_config().unwrap_err();

    // Then
    assert_invalid_configuration_error(&error);
    assert_eq!(files.reads.get(), 1);
    assert_eq!(files.lock_observations.get(), 0);
    assert_eq!(provider.validations.get(), 0);
    assert_eq!(provider.embeddings.get(), 0);

    // Given: a syntactically valid configuration rejected by local provider validation.
    let files = Files {
        config: Some(CONFIG.to_vec()),
        is_failing: false,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::rejecting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let error = service.get_config().unwrap_err();

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
        is_failing: true,
        reads: Cell::new(0),
        lock_observations: Cell::new(0),
    };
    let provider = Provider::accepting();
    let service = ConfigurationService {
        file_reader: &files,
        provider: &provider,
    };

    // When
    let error = service.get_config().unwrap_err();

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

fn assert_invalid_configuration_error(error: &CommandError) {
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
    is_failing: bool,
    reads: Cell<usize>,
    lock_observations: Cell<usize>,
}

impl FileReader for Files {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        panic!("unexpected file_get_by_path")
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        assert_eq!(path, std::path::Path::new(".letopis/config.json"));
        self.reads.set(self.reads.get() + 1);
        if self.is_failing {
            return Err(FileError::new_with_cause(
                FileErrorKind::Io,
                io::Error::other("CONFIG_CANARY"),
            ));
        }
        Ok(self.config.clone())
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        panic!("unexpected file_get_stat_by_path")
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unexpected file_get_identity_by_path")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unexpected file_is_exist")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_identity_by_path")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_distinct_by_paths")
    }

    fn directory_get_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::os::fd::OwnedFd>> {
        Ok(None)
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        Ok(None)
    }
}

struct Provider {
    is_rejecting: bool,
    validations: Cell<usize>,
    embeddings: Cell<usize>,
}

impl Provider {
    fn accepting() -> Self {
        Self {
            is_rejecting: false,
            validations: Cell::new(0),
            embeddings: Cell::new(0),
        }
    }

    fn rejecting() -> Self {
        Self {
            is_rejecting: true,
            validations: Cell::new(0),
            embeddings: Cell::new(0),
        }
    }
}

impl EmbeddingProvider for Provider {
    fn config_validate(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
        self.validations.set(self.validations.get() + 1);
        if self.is_rejecting {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
        }
        Ok(())
    }

    fn vectors_compute_by_texts(
        &self,
        _: &EmbeddingConfig,
        _: &[&str],
        _: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.embeddings.set(self.embeddings.get() + 1);
        Ok(Vec::new())
    }
}
