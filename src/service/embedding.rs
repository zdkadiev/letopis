use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigState, DrainOutcome, EmbeddingConfig,
    EmbeddingError, EmbeddingErrorKind, EmbeddingFiles, EmbeddingProvider, EmbeddingResult,
    EmbeddingTasks, EmbeddingWork, Embeddings, Markdown, QueryEmbedding, QueryEmbeddingRequest,
    Storage, StorageError, StorageErrorKind, StorageRead, VectorOutcome, VectorWrite,
};
use crate::domain::{KnowledgeId, RepoPath, Sha256};
use std::{collections::BTreeSet, num::NonZeroUsize, time::Duration};

const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Coordinates ready configuration, durable document work and transient queries.
#[derive(Debug)]
pub(crate) struct EmbeddingService<S, F, M, P> {
    storage: S,
    files: F,
    markdown: M,
    provider: P,
}
impl<S: Storage, F: EmbeddingFiles, M: Markdown, P: EmbeddingProvider>
    EmbeddingService<S, F, M, P>
{
    /// Construct the application service from its owned boundary implementations.
    pub(crate) fn new(storage: S, files: F, markdown: M, provider: P) -> Self {
        Self {
            storage,
            files,
            markdown,
            provider,
        }
    }
}

impl<S: Storage, F: EmbeddingFiles, M: Markdown, P: EmbeddingProvider> EmbeddingTasks
    for EmbeddingService<S, F, M, P>
{
    fn has_work(&mut self) -> CommandResult<bool> {
        let ConfigState::Ready(config) = self.read_configuration().map_err(map_command)? else {
            return Ok(false);
        };
        let next = self
            .storage
            .begin_read()
            .map_err(map_storage)
            .map_err(map_command)?
            .read_next_embedding_work(&config.get_identity(), config.get_max_attempts(), None)
            .map_err(map_storage)
            .map_err(map_command)?;
        Ok(matches!(next, VectorOutcome::Current(Some(_))))
    }

    fn drain(&mut self) -> CommandResult<()> {
        match Embeddings::drain(self).map_err(map_command)? {
            DrainOutcome::Complete
            | DrainOutcome::AlreadyRunning
            | DrainOutcome::Unavailable
            | DrainOutcome::ConfigChanged => Ok(()),
        }
    }
}

impl<S: Storage, F: EmbeddingFiles, M: Markdown, P: EmbeddingProvider> Embeddings
    for EmbeddingService<S, F, M, P>
{
    fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding> {
        let ConfigState::Ready(config) = self.read_configuration()? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        let VectorOutcome::Current(dimensions) = self.read_dimensions(&config)? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        self.embed_query_with_config(QueryEmbeddingRequest {
            text,
            config: &config,
            dimensions,
        })
    }

    fn embed_query_with_config(
        &mut self,
        request: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding> {
        if self.provider.validate_config(request.config).is_err() {
            return Ok(QueryEmbedding::Unavailable);
        }
        let VectorOutcome::Current(_) = self.read_dimensions(request.config)? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        let mut result = self.compute_vector(request.config, request.text, request.dimensions);
        let VectorOutcome::Current(current) = self.read_dimensions(request.config)? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        if let Ok(vector) = &result
            && current.is_some_and(|size| vector.len() != size.get())
        {
            result = Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        self.publish_query(request.config, result)
    }

    fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
        let ConfigState::Ready(config) = self.read_configuration()? else {
            return Ok(DrainOutcome::Unavailable);
        };
        let Some(_guard) = self.files.try_lock_embed()? else {
            return Ok(DrainOutcome::AlreadyRunning);
        };
        self.drain_ready(&config)
    }

    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        match self.read_ready_config() {
            Ok(None) => Ok(ConfigState::Missing),
            Err(error) => Ok(ConfigState::Invalid(error.get_kind())),
            Ok(Some(config)) => {
                self.storage
                    .reset_vectors(&config.get_identity())
                    .map_err(map_storage)?;
                Ok(ConfigState::Ready(config))
            }
        }
    }
}

impl<S: Storage, F: EmbeddingFiles, M: Markdown, P: EmbeddingProvider>
    EmbeddingService<S, F, M, P>
{
    fn read_ready_config(&self) -> EmbeddingResult<Option<EmbeddingConfig>> {
        self.files
            .read_config()?
            .map(|bytes| {
                let config = EmbeddingConfig::parse(&bytes)?;
                self.provider.validate_config(&config)?;
                Ok(config)
            })
            .transpose()
    }

    fn read_dimensions(
        &mut self,
        config: &EmbeddingConfig,
    ) -> EmbeddingResult<VectorOutcome<Option<NonZeroUsize>>> {
        self.storage
            .begin_read()
            .map_err(map_storage)?
            .read_vector_dimensions(&config.get_identity())
            .map_err(map_storage)
    }

    fn prepare_text(&self, work: &EmbeddingWork) -> EmbeddingResult<Option<String>> {
        #[cfg(test)]
        tests::reach_checkpoint("prepare");
        let path = RepoPath::parse(work.id.as_str().split('#').next().unwrap_or_default())
            .map_err(|cause| {
                EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
            })?;
        let Some(bytes) = self.files.read_document(&path)? else {
            return Ok(None);
        };
        let parsed = self
            .markdown
            .parse_file(&path, &bytes)
            .map_err(|_| EmbeddingError::new(EmbeddingErrorKind::Input))?;
        Ok(parsed
            .and_then(|file| {
                file.elements.into_iter().find(|element| {
                    element.element.id == work.id && element.search_hash == work.search_hash
                })
            })
            .map(|element| element.search_text))
    }

    fn reserve(
        &mut self,
        config: &EmbeddingConfig,
        hash: &Sha256,
    ) -> EmbeddingResult<VectorOutcome<bool>> {
        let VectorOutcome::Current(mut unit) = self
            .storage
            .begin_vector_write(&config.get_identity())
            .map_err(map_storage)?
        else {
            return Ok(VectorOutcome::ConfigChanged);
        };
        let reserved = unit
            .reserve_attempt(hash, config.get_max_attempts())
            .map_err(map_storage)?;
        unit.commit().map_err(map_storage)?;
        Ok(VectorOutcome::Current(reserved))
    }

    fn record_failure(
        &mut self,
        config: &EmbeddingConfig,
        hash: &Sha256,
        error: &EmbeddingError,
    ) -> EmbeddingResult<VectorOutcome<()>> {
        let VectorOutcome::Current(mut unit) = self
            .storage
            .begin_vector_write(&config.get_identity())
            .map_err(map_storage)?
        else {
            return Ok(VectorOutcome::ConfigChanged);
        };
        unit.record_failure(hash, &error.to_string())
            .map_err(map_storage)?;
        unit.commit().map_err(map_storage)?;
        Ok(VectorOutcome::Current(()))
    }

    fn compute_vector(
        &self,
        config: &EmbeddingConfig,
        text: &str,
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<f32>> {
        let requested = config.get_dimensions().or(dimensions);
        let mut vectors = self.provider.embed(config, &[text], requested)?;
        if vectors.len() != 1 {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        let vector = vectors.remove(0);
        if vector.is_empty()
            || vector.iter().any(|value| !value.is_finite())
            || requested.is_some_and(|size| vector.len() != size.get())
            || dimensions.is_some_and(|size| vector.len() != size.get())
        {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        Ok(vector)
    }

    fn publish_vector(
        &mut self,
        config: &EmbeddingConfig,
        hash: &Sha256,
        vector: &[f32],
    ) -> EmbeddingResult<VectorOutcome<()>> {
        let VectorOutcome::Current(mut unit) = self
            .storage
            .begin_vector_write(&config.get_identity())
            .map_err(map_storage)?
        else {
            return Ok(VectorOutcome::ConfigChanged);
        };
        match unit.store_vector(hash, vector, vector.len()) {
            Ok(_) => {
                unit.commit().map_err(map_storage)?;
                Ok(VectorOutcome::Current(()))
            }
            Err(error) if error.get_kind() == StorageErrorKind::InvalidRequest => {
                drop(unit);
                Err(EmbeddingError::new_with_cause(
                    EmbeddingErrorKind::InvalidResponse,
                    error,
                ))
            }
            Err(error) => Err(map_storage(error)),
        }
    }

    fn process_work(
        &mut self,
        config: &EmbeddingConfig,
        work: &EmbeddingWork,
        text: EmbeddingResult<String>,
    ) -> EmbeddingResult<DrainOutcome> {
        let VectorOutcome::Current(dimensions) = self.read_dimensions(config)? else {
            return Ok(DrainOutcome::ConfigChanged);
        };
        match self.reserve(config, &work.search_hash)? {
            VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
            VectorOutcome::Current(false) => return Ok(DrainOutcome::Complete),
            VectorOutcome::Current(true) => {}
        }
        #[cfg(test)]
        tests::reach_checkpoint("reserved");
        let text = match text {
            Ok(text) => text,
            Err(error) => return self.finish_failure(config, &work.search_hash, &error),
        };
        let vector = match self.compute_vector(config, &text, dimensions) {
            Ok(vector) => vector,
            Err(error) => return self.finish_failure(config, &work.search_hash, &error),
        };
        match self.publish_vector(config, &work.search_hash, &vector) {
            Ok(VectorOutcome::ConfigChanged) => Ok(DrainOutcome::ConfigChanged),
            Ok(VectorOutcome::Current(())) => Ok(DrainOutcome::Complete),
            Err(error) if error.get_kind() == EmbeddingErrorKind::InvalidResponse => {
                self.finish_failure(config, &work.search_hash, &error)
            }
            Err(error) => Err(error),
        }
    }

    fn finish_failure(
        &mut self,
        config: &EmbeddingConfig,
        hash: &Sha256,
        error: &EmbeddingError,
    ) -> EmbeddingResult<DrainOutcome> {
        if matches!(
            self.record_failure(config, hash, error)?,
            VectorOutcome::ConfigChanged
        ) {
            return Ok(DrainOutcome::ConfigChanged);
        }
        let failure = self
            .storage
            .begin_read()
            .map_err(map_storage)?
            .read_failure(&config.get_identity(), hash)
            .map_err(map_storage)?;
        match failure {
            VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
            VectorOutcome::Current(Some(failure))
                if failure.attempts < config.get_max_attempts() =>
            {
                self.files.wait(RETRY_DELAY)?;
            }
            VectorOutcome::Current(_) => {}
        }
        Ok(DrainOutcome::Complete)
    }

    fn drain_ready(&mut self, config: &EmbeddingConfig) -> EmbeddingResult<DrainOutcome> {
        let mut skipped = BTreeSet::<KnowledgeId>::new();
        let mut after = None;
        loop {
            let next = self
                .storage
                .begin_read()
                .map_err(map_storage)?
                .read_next_embedding_work(
                    &config.get_identity(),
                    config.get_max_attempts(),
                    after.as_ref(),
                )
                .map_err(map_storage)?;
            let work = match next {
                VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
                VectorOutcome::Current(None) => return self.prune(config),
                VectorOutcome::Current(Some(work)) => work,
            };
            if skipped.contains(&work.id) {
                after = Some(work.id);
                continue;
            }
            let prepared = match self.prepare_text(&work) {
                Ok(None) => {
                    after = Some(work.id.clone());
                    skipped.insert(work.id);
                    continue;
                }
                Ok(Some(text)) => Ok(text),
                Err(error) => Err(error),
            };
            if self.process_work(config, &work, prepared)? == DrainOutcome::ConfigChanged {
                return Ok(DrainOutcome::ConfigChanged);
            }
            after = None;
        }
    }

    fn prune(&mut self, config: &EmbeddingConfig) -> EmbeddingResult<DrainOutcome> {
        let VectorOutcome::Current(mut unit) = self
            .storage
            .begin_vector_write(&config.get_identity())
            .map_err(map_storage)?
        else {
            return Ok(DrainOutcome::ConfigChanged);
        };
        unit.prune_unreferenced().map_err(map_storage)?;
        unit.commit().map_err(map_storage)?;
        Ok(DrainOutcome::Complete)
    }

    fn publish_query(
        &mut self,
        config: &EmbeddingConfig,
        result: EmbeddingResult<Vec<f32>>,
    ) -> EmbeddingResult<QueryEmbedding> {
        let result = match result {
            Ok(vector) => Ok(vector),
            Err(error) => Err((self.files.get_time_secs()?, error)),
        };
        let VectorOutcome::Current(mut unit) = self
            .storage
            .begin_vector_write(&config.get_identity())
            .map_err(map_storage)?
        else {
            return Ok(QueryEmbedding::Unavailable);
        };
        let outcome = match result {
            Ok(vector) => {
                unit.clear_query_error().map_err(map_storage)?;
                QueryEmbedding::Available {
                    identity: config.get_identity(),
                    vector,
                }
            }
            Err((at, error)) => {
                unit.set_query_error(at, &error.to_string())
                    .map_err(map_storage)?;
                QueryEmbedding::Unavailable
            }
        };
        unit.commit().map_err(map_storage)?;
        Ok(outcome)
    }
}

fn map_command(error: EmbeddingError) -> CommandError {
    let kind = if error.get_kind() == EmbeddingErrorKind::InternalError {
        CommandErrorKind::InternalError
    } else {
        CommandErrorKind::EmbeddingFailed
    };
    CommandError::new_with_cause(kind, "embedding operation failed", error)
}

fn map_storage(error: StorageError) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
}

#[cfg(test)]
mod tests;
