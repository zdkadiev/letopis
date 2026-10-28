use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigReader, EmbeddingConfig, EmbeddingError,
    EmbeddingProvider, StatusFiles,
};
use std::fmt;

const INVALID_CONFIG: &str = "invalid embedding configuration";
const READ_FAILURE: &str = "cannot read embedding configuration";

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
    fn read_config(&self) -> CommandResult<Option<EmbeddingConfig>> {
        let Some(bytes) = self.files.read_config().map_err(map_read_error)? else {
            return Ok(None);
        };
        let config = EmbeddingConfig::parse(&bytes).map_err(map_invalid_config)?;
        self.provider
            .validate_config(&config)
            .map_err(map_invalid_config)?;
        Ok(Some(config))
    }
}

fn map_read_error(cause: EmbeddingError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, READ_FAILURE, cause)
}

fn map_invalid_config(cause: EmbeddingError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::EmbeddingFailed, INVALID_CONFIG, cause)
}

#[cfg(test)]
mod tests;
