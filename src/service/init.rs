use super::utils::map_error_to_config_read_command_error;
use super::{
    CommandError, CommandErrorKind, CommandResult, Configuration, EmbeddingProvider,
    EmbeddingResult, FileWriter, GENERATED_CONFIG, GENERATED_IGNORE, InitStorage, Initializer,
    map_configuration_error_to_command_error,
};
use crate::service::FileResult;
use crate::utils::{CONFIG_PATH, IGNORE_PATH, SERVICE_DIRECTORY};
use std::path::Path;
use std::{fmt, path::PathBuf};

const INIT_FAILURE: &str = "cannot initialize repository";
const STORAGE_FAILURE: &str = "cannot initialize repository storage";

/// Initializes repository schemas and configuration once.
pub(crate) struct InitService {
    file_writer: Box<dyn FileWriter>,
    init_storage: Box<dyn InitStorage>,
    provider: Box<dyn EmbeddingProvider>,
    directory: PathBuf,
}

impl InitService {
    /// Creates the service from its owned collaborators.
    ///
    /// # Arguments
    ///
    /// * `file_writer` — File writer used to create the service directory and configuration.
    /// * `init_storage` — Initializer that creates repository storage under the target directory.
    /// * `provider` — Embedding provider used to validate the published configuration.
    /// * `directory` — Repository directory where storage is initialized.
    pub(crate) fn new(
        file_writer: Box<dyn FileWriter>,
        init_storage: Box<dyn InitStorage>,
        provider: Box<dyn EmbeddingProvider>,
        directory: PathBuf,
    ) -> Self {
        Self {
            file_writer,
            init_storage,
            provider,
            directory,
        }
    }

    fn get_config_bytes(&self) -> FileResult<Option<Vec<u8>>> {
        super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))?;
            self.file_writer
                .file_get_bytes_by_path(Path::new(CONFIG_PATH))
        })
    }
}

impl fmt::Debug for InitService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InitService")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl Initializer for InitService {
    fn initialize(&mut self) -> CommandResult<()> {
        self.verify_existing_configuration()?;
        let pinned = super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_error_to_init_command_error)?;
        self.init_storage
            .initialize(&self.directory, &pinned)
            .map_err(map_error_to_init_storage_command_error)?;
        super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .file_create_if_absent_by_path(Path::new(CONFIG_PATH), GENERATED_CONFIG)
        })
        .map_err(map_error_to_init_command_error)?;
        super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .file_create_if_absent_by_path(Path::new(IGNORE_PATH), GENERATED_IGNORE)
        })
        .map_err(map_error_to_init_command_error)?;
        let bytes = self
            .get_config_bytes()
            .map_err(map_error_to_config_read_command_error)?
            .ok_or_else(build_init_error)?;
        self.verify_configuration_bytes(&bytes)?;
        self.sync_service_directories()
    }
}

impl InitService {
    /// Verifies the stored configuration when one already exists.
    ///
    /// # Errors
    /// Reports unreadable or invalid configuration before any state is written.
    fn verify_existing_configuration(&self) -> CommandResult<()> {
        let Some(bytes) = self
            .get_config_bytes()
            .map_err(map_error_to_config_read_command_error)?
        else {
            return Ok(());
        };
        self.verify_configuration_bytes(&bytes)
    }

    /// Verifies the stored configuration bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` — Configuration bytes that must parse and validate.
    ///
    /// # Errors
    /// Reports invalid or unsupported configuration as a command failure.
    fn verify_configuration_bytes(&self, bytes: &[u8]) -> CommandResult<()> {
        let configuration =
            Configuration::parse(bytes).map_err(map_configuration_error_to_command_error)?;
        self.validate_configuration(&configuration)
            .map_err(map_configuration_error_to_command_error)
    }

    /// Syncs the service directory and repository root after publication.
    ///
    /// # Errors
    /// Reports directory sync failures with the initialization message.
    fn sync_service_directories(&self) -> CommandResult<()> {
        super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .directory_sync_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_error_to_init_command_error)?;
        super::utils::retry_changed_file_observation(|| {
            self.file_writer.directory_sync_by_path(Path::new("."))
        })
        .map_err(map_error_to_init_command_error)?;
        super::utils::retry_changed_file_observation(|| {
            self.file_writer
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_error_to_init_command_error)?
        .ok_or_else(build_init_error)
        .map(|_| ())
    }

    fn validate_configuration(&self, configuration: &Configuration) -> EmbeddingResult<()> {
        let Some(config) = configuration.get_embedding_config() else {
            return Ok(());
        };
        self.provider.config_validate(config)
    }
}

/// Converts an error to an initialization command error.
///
/// # Arguments
///
/// * `cause` — Failure from an initialization step.
fn map_error_to_init_command_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, INIT_FAILURE, cause)
}

/// Converts a storage failure to an initialization storage command error.
///
/// # Arguments
///
/// * `cause` — Storage failure from initialization.
fn map_error_to_init_storage_command_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, STORAGE_FAILURE, cause)
}

/// Builds the error reported when initialization cannot complete.
fn build_init_error() -> CommandError {
    CommandError::new(CommandErrorKind::InternalError, INIT_FAILURE)
}

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
