use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigReader, Configuration, EmbeddingError,
    EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult, FileReader,
};
use crate::service::FileResult;
use crate::utils::{CONFIG_PATH, SERVICE_DIRECTORY};
use std::fmt;
use std::path::Path;

const INVALID_CONFIG: &str = "invalid embedding configuration";
const MIGRATION_REQUIRED: &str = "embedding configuration requires migration";

/// Observes the current validated embedding configuration without changing state.
pub(crate) struct ConfigurationService<'a> {
    pub(crate) file_reader: &'a dyn FileReader,
    pub(crate) provider: &'a dyn EmbeddingProvider,
}

impl fmt::Debug for ConfigurationService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigurationService")
            .finish_non_exhaustive()
    }
}

impl ConfigReader for ConfigurationService<'_> {
    fn get_config(&self) -> CommandResult<Option<Configuration>> {
        let Some(bytes) = self.get_config_bytes().map_err(|error| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                super::utils::CONFIG_READ_FAILURE,
                error,
            )
        })?
        else {
            return Ok(None);
        };
        let configuration = Configuration::parse(&bytes).map_err(map_invalid_configuration)?;
        self.validate_configuration(&configuration)
            .map_err(map_invalid_configuration)?;
        Ok(Some(configuration))
    }
}

impl ConfigurationService<'_> {
    fn get_config_bytes(&self) -> FileResult<Option<Vec<u8>>> {
        super::utils::retry_changed_file_observation(|| {
            self.file_reader
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))?;
            self.file_reader
                .file_get_bytes_by_path(Path::new(CONFIG_PATH))
        })
    }

    fn validate_configuration(&self, configuration: &Configuration) -> EmbeddingResult<()> {
        let Some(config) = configuration.get_embedding_config() else {
            return Ok(());
        };
        self.provider.config_validate(config)
    }
}

pub(crate) fn map_invalid_configuration(cause: EmbeddingError) -> CommandError {
    let message = if cause.get_kind() == EmbeddingErrorKind::MigrationRequired {
        MIGRATION_REQUIRED
    } else {
        INVALID_CONFIG
    };
    CommandError::new_with_cause(CommandErrorKind::EmbeddingFailed, message, cause)
}

#[cfg(test)]
#[path = "configuration_test.rs"]
mod tests;
