use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigReader, Configuration, EmbeddingError,
    EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult, StatusFiles,
};
use std::fmt;

const INVALID_CONFIG: &str = "invalid embedding configuration";
const MIGRATION_REQUIRED: &str = "embedding configuration requires migration";
const READ_FAILURE: &str = "cannot read embedding configuration";

/// Parse one complete configuration document and validate retained provider
/// settings locally, including when execution is disabled.
///
/// # Errors
/// Returns the structural parser error, or the provider's local validation error.
pub(crate) fn validate_configuration(
    bytes: &[u8],
    provider: &dyn EmbeddingProvider,
) -> EmbeddingResult<Configuration> {
    let configuration = Configuration::parse(bytes)?;
    if let Some(config) = configuration.get_embedding_config() {
        provider.validate_config(config)?;
    }
    Ok(configuration)
}

/// Observes the current validated embedding configuration without changing state.
pub(crate) struct ConfigurationService<'a> {
    pub(crate) files: &'a dyn StatusFiles,
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
        let Some(bytes) = self.files.get_config().map_err(map_read_error)? else {
            return Ok(None);
        };
        validate_configuration(&bytes, self.provider)
            .map(Some)
            .map_err(map_invalid_configuration)
    }
}

pub(crate) fn map_read_error(cause: EmbeddingError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, READ_FAILURE, cause)
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
mod tests;
