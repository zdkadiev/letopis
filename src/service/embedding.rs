use super::{
    CacheDatabase, Clock, CommandError, CommandErrorKind, CommandResult, ConfigState,
    Configuration, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind,
    EmbeddingExecutor, EmbeddingProvider, EmbeddingResult, EmbeddingWork, FileLocker, FileReader,
    MarkdownCodec, QueryEmbedding, QueryEmbeddingRequest, StorageError, StorageErrorKind,
    TransactionError, TransactionRequirement, VectorDatabase, VectorOutcome,
};
use crate::domain::{KnowledgeId, RepoPath, Sha256};
use crate::service::{FileError, FileErrorKind};
use crate::utils::{CONFIG_PATH, EMBED_LOCK_PATH, SERVICE_DIRECTORY};
use std::error::Error;
use std::path::Path;
use std::{collections::BTreeSet, num::NonZeroUsize, rc::Rc, time::Duration};

const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Coordinates ready configuration, durable document work and transient queries.
#[derive(Debug)]
pub(crate) struct EmbeddingService<C, V, F, M, P> {
    // Release the read-only cache attachment before the final writable cache owner.
    vector_database: Rc<V>,
    cache: Rc<C>,
    file_reader: F,
    clock: Box<dyn Clock>,
    markdown: M,
    provider: P,
}

impl<
    C: CacheDatabase,
    V: VectorDatabase,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: EmbeddingProvider,
> EmbeddingService<C, V, F, M, P>
{
    /// Creates the service from its owned collaborators.
    ///
    /// # Arguments
    ///
    /// * `cache` — Shared cache used for document work.
    /// * `vector_database` — Shared vector store for embeddings and failure records.
    /// * `file_reader` — File reader that also acquires the index exclusion lock.
    /// * `clock` — Clock used for retry waits and query failure timestamps.
    /// * `markdown` — Markdown codec used to parse element search text.
    /// * `provider` — Embedding provider used to validate configuration and compute vectors.
    pub(crate) fn new(
        cache: Rc<C>,
        vector_database: Rc<V>,
        file_reader: F,
        clock: Box<dyn Clock>,
        markdown: M,
        provider: P,
    ) -> Self {
        Self {
            vector_database,
            cache,
            file_reader,
            clock,
            markdown,
            provider,
        }
    }
}

impl<
    C: CacheDatabase,
    V: VectorDatabase,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: EmbeddingProvider,
> EmbeddingExecutor for EmbeddingService<C, V, F, M, P>
{
    fn is_work_available(&mut self) -> CommandResult<bool> {
        let ConfigState::Ready(config) = self
            .get_configuration()
            .map_err(map_embedding_error_to_command_error)?
        else {
            return Ok(false);
        };
        let next = self
            .cache
            .with_read_tx(|| {
                self.cache
                    .nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
                        &config.get_identity(),
                        config.get_max_attempts(),
                        None,
                    )
            })
            .map_err(map_storage_transaction_error_to_embedding_error)
            .map_err(map_embedding_error_to_command_error)?;
        Ok(matches!(next, VectorOutcome::Current(Some(_))))
    }

    fn drain(&mut self) -> CommandResult<()> {
        match Embedder::drain(self).map_err(map_embedding_error_to_command_error)? {
            DrainOutcome::Complete
            | DrainOutcome::AlreadyRunning
            | DrainOutcome::Unavailable
            | DrainOutcome::ConfigChanged => Ok(()),
        }
    }
}

impl<
    C: CacheDatabase,
    V: VectorDatabase,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: EmbeddingProvider,
> Embedder for EmbeddingService<C, V, F, M, P>
{
    fn get_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        let observed = self
            .cache
            .with_read_tx(|| self.cache.meta_get_by_id())
            .map_err(map_storage_transaction_error_to_embedding_error)?;
        #[cfg(test)]
        tests::reach_checkpoint("observed");
        let configuration = match self.get_configuration_by_file() {
            Ok(None) => return Ok(ConfigState::Missing),
            Err(error) => return Ok(ConfigState::Invalid(error.get_kind())),
            Ok(Some(configuration)) => {
                if let Err(error) = self.validate_configuration(&configuration) {
                    return Ok(ConfigState::Invalid(error.get_kind()));
                }
                configuration
            }
        };
        if !configuration.is_enabled() {
            return Ok(ConfigState::Disabled);
        }
        let Some(config) = configuration.into_embedding_config() else {
            return Ok(ConfigState::Disabled);
        };
        #[cfg(test)]
        tests::reach_checkpoint("configuration");
        match self
            .vector_database
            .reset_vectors(observed.as_ref(), &config.get_identity())
            .map_err(map_error_to_embedding_internal_error)?
        {
            VectorOutcome::Current(_) => Ok(ConfigState::Ready(config)),
            VectorOutcome::ConfigChanged => Ok(ConfigState::Changed),
        }
    }

    fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
        self.verify_drain_scope()?;
        let config = match self.get_configuration()? {
            ConfigState::Ready(config) => config,
            ConfigState::Changed => return Ok(DrainOutcome::ConfigChanged),
            ConfigState::Missing | ConfigState::Invalid(_) | ConfigState::Disabled => {
                return Ok(DrainOutcome::Unavailable);
            }
        };
        let Some(_guard) = self
            .file_reader
            .lock_try_acquire_by_path(Path::new(EMBED_LOCK_PATH))
            .map_err(map_error_to_embedding_internal_error)?
        else {
            return Ok(DrainOutcome::AlreadyRunning);
        };
        self.drain_ready(&config)
    }

    fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding> {
        self.verify_query_scope()?;
        let ConfigState::Ready(config) = self.get_configuration()? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        let VectorOutcome::Current(dimensions) = self.get_dimensions_by_config(&config)? else {
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
        self.verify_query_scope()?;
        if self.provider.config_validate(request.config).is_err() {
            return Ok(QueryEmbedding::Unavailable);
        }
        let VectorOutcome::Current(_) = self.get_dimensions_by_config(request.config)? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        let mut result = self.compute_vector(request.config, request.text, request.dimensions);
        let VectorOutcome::Current(current) = self.get_dimensions_by_config(request.config)? else {
            return Ok(QueryEmbedding::Unavailable);
        };
        if let Ok(vector) = &result
            && current.is_some_and(|size| vector.len() != size.get())
        {
            result = Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        self.publish_query(request.config, result)
    }
}

impl<
    C: CacheDatabase,
    V: VectorDatabase,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: EmbeddingProvider,
> EmbeddingService<C, V, F, M, P>
{
    fn verify_query_scope(&self) -> EmbeddingResult<()> {
        self.cache
            .verify_transaction_scope(TransactionRequirement::ReadOnly)
            .map_err(map_error_to_embedding_internal_error)?;
        self.vector_database
            .verify_transaction_scope(TransactionRequirement::Inactive)
            .map_err(map_error_to_embedding_internal_error)
    }

    fn verify_drain_scope(&self) -> EmbeddingResult<()> {
        self.cache
            .verify_transaction_scope(TransactionRequirement::Inactive)
            .map_err(map_error_to_embedding_internal_error)?;
        self.vector_database
            .verify_transaction_scope(TransactionRequirement::Inactive)
            .map_err(map_error_to_embedding_internal_error)
    }

    fn get_configuration_by_file(&self) -> EmbeddingResult<Option<Configuration>> {
        self.file_reader
            .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
            .map_err(map_error_to_embedding_internal_error)?;
        let bytes = self
            .file_reader
            .file_get_bytes_by_path(Path::new(CONFIG_PATH));
        // A directory replacement remains infrastructure failure even if the read also failed.
        self.file_reader
            .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
            .map_err(map_error_to_embedding_internal_error)?;
        bytes
            .map_err(map_file_error_to_embedding_error)?
            .map(|bytes| Configuration::parse(&bytes))
            .transpose()
    }

    fn validate_configuration(&self, configuration: &Configuration) -> EmbeddingResult<()> {
        let Some(config) = configuration.get_embedding_config() else {
            return Ok(());
        };
        self.provider.config_validate(config)
    }

    fn drain_ready(&mut self, config: &EmbeddingConfig) -> EmbeddingResult<DrainOutcome> {
        let mut skipped = BTreeSet::<KnowledgeId>::new();
        let mut after = None;
        loop {
            let next = self
                .cache
                .with_read_tx(|| {
                    self.cache
                        .nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
                            &config.get_identity(),
                            config.get_max_attempts(),
                            after.as_ref(),
                        )
                })
                .map_err(map_storage_transaction_error_to_embedding_error)?;
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

    fn prune(&self, config: &EmbeddingConfig) -> EmbeddingResult<DrainOutcome> {
        let outcome = self
            .vector_database
            .with_write_tx(&config.get_identity(), || {
                self.vector_database.prune_unreferenced()
            })
            .map_err(map_storage_transaction_error_to_embedding_error)?;
        Ok(match outcome {
            VectorOutcome::Current(()) => DrainOutcome::Complete,
            VectorOutcome::ConfigChanged => DrainOutcome::ConfigChanged,
        })
    }

    fn prepare_text(&self, work: &EmbeddingWork) -> EmbeddingResult<Option<String>> {
        #[cfg(test)]
        tests::reach_checkpoint("prepare");
        let path = RepoPath::parse(work.id.as_str().split('#').next().unwrap_or_default())
            .map_err(map_error_to_embedding_internal_error)?;
        let Some(bytes) = self
            .file_reader
            .file_get_bytes_by_path(Path::new(path.as_str()))
            .map_err(map_file_error_to_embedding_error)?
        else {
            return Ok(None);
        };
        let parsed = self
            .markdown
            .file_parse(&path, &bytes)
            .map_err(|_| EmbeddingError::new(EmbeddingErrorKind::Input))?;
        Ok(parsed
            .and_then(|file| {
                file.elements.into_iter().find(|element| {
                    element.element.id == work.id && element.search_hash == work.search_hash
                })
            })
            .map(|element| element.search_text))
    }

    fn process_work(
        &mut self,
        config: &EmbeddingConfig,
        work: &EmbeddingWork,
        text: EmbeddingResult<String>,
    ) -> EmbeddingResult<DrainOutcome> {
        let VectorOutcome::Current(dimensions) = self.get_dimensions_by_config(config)? else {
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

    fn get_dimensions_by_config(
        &self,
        config: &EmbeddingConfig,
    ) -> EmbeddingResult<VectorOutcome<Option<NonZeroUsize>>> {
        self.vector_database
            .with_read_tx(&config.get_identity(), || {
                self.vector_database.vector_index_config_get_by_id()
            })
            .map_err(map_storage_transaction_error_to_embedding_error)
    }

    fn reserve(
        &self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
    ) -> EmbeddingResult<VectorOutcome<bool>> {
        self.vector_database
            .with_write_tx(&config.get_identity(), || {
                self.vector_database
                    .embed_failures_upsert_by_search_hash_and_attempts(
                        search_hash,
                        config.get_max_attempts(),
                    )
            })
            .map_err(map_storage_transaction_error_to_embedding_error)
    }

    fn finish_failure(
        &mut self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
        error: &EmbeddingError,
    ) -> EmbeddingResult<DrainOutcome> {
        if matches!(
            self.record_failure(config, search_hash, error)?,
            VectorOutcome::ConfigChanged
        ) {
            return Ok(DrainOutcome::ConfigChanged);
        }
        let failure = self
            .vector_database
            .with_read_tx(&config.get_identity(), || {
                self.vector_database
                    .embed_failures_get_by_search_hash(search_hash)
            })
            .map_err(map_storage_transaction_error_to_embedding_error)?;
        match failure {
            VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
            VectorOutcome::Current(Some(failure))
                if failure.attempts < config.get_max_attempts() =>
            {
                self.clock
                    .clock_wait(RETRY_DELAY)
                    .map_err(map_error_to_embedding_internal_error)?;
            }
            VectorOutcome::Current(_) => {}
        }
        Ok(DrainOutcome::Complete)
    }

    fn record_failure(
        &self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
        error: &EmbeddingError,
    ) -> EmbeddingResult<VectorOutcome<()>> {
        self.vector_database
            .with_write_tx(&config.get_identity(), || {
                self.vector_database
                    .embed_failures_update_by_search_hash(search_hash, &error.to_string())
                    .map(|_| ())
            })
            .map_err(map_storage_transaction_error_to_embedding_error)
    }

    fn compute_vector(
        &self,
        config: &EmbeddingConfig,
        text: &str,
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<f32>> {
        let requested = config.get_dimensions_by_parameters().or(dimensions);
        let mut vector_database =
            self.provider
                .vectors_compute_by_texts(config, &[text], requested)?;
        if vector_database.len() != 1 {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidResponse));
        }
        let vector = vector_database.remove(0);
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
        &self,
        config: &EmbeddingConfig,
        search_hash: &Sha256,
        vector: &[f32],
    ) -> EmbeddingResult<VectorOutcome<()>> {
        self.vector_database
            .with_write_tx(&config.get_identity(), || {
                self.vector_database
                    .store_vector(search_hash, vector, vector.len())
                    .map(|_| ())
                    .map_err(map_storage_error_to_embedding_invalid_response_error)
            })
            .map_err(map_embedding_transaction_error_to_embedding_error)
    }

    fn publish_query(
        &self,
        config: &EmbeddingConfig,
        result: EmbeddingResult<Vec<f32>>,
    ) -> EmbeddingResult<QueryEmbedding> {
        let result = match result {
            Ok(vector) => Ok(vector),
            Err(error) => Err((
                self.clock
                    .clock_get_time_secs()
                    .map_err(map_error_to_embedding_internal_error)?,
                error,
            )),
        };
        let outcome = self
            .vector_database
            .with_write_tx(
                &config.get_identity(),
                || -> EmbeddingResult<QueryEmbedding> {
                    match result {
                        Ok(vector) => {
                            self.vector_database
                                .query_error_delete_by_id()
                                .map_err(map_error_to_embedding_internal_error)?;
                            Ok(QueryEmbedding::Available {
                                identity: config.get_identity(),
                                vector,
                            })
                        }
                        Err((at, error)) => {
                            self.vector_database
                                .query_error_upsert_by_id(at, &error.to_string())
                                .map_err(map_error_to_embedding_internal_error)?;
                            Ok(QueryEmbedding::Unavailable)
                        }
                    }
                },
            )
            .map_err(map_embedding_transaction_error_to_embedding_error)?;
        Ok(match outcome {
            VectorOutcome::Current(result) => result,
            VectorOutcome::ConfigChanged => QueryEmbedding::Unavailable,
        })
    }
}

fn map_storage_transaction_error_to_embedding_error(
    error: TransactionError<StorageError>,
) -> EmbeddingError {
    match error {
        TransactionError::Operation(error) | TransactionError::Storage(error) => {
            EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
        }
    }
}

fn map_embedding_transaction_error_to_embedding_error(
    error: TransactionError<EmbeddingError>,
) -> EmbeddingError {
    match error {
        TransactionError::Operation(error) => error,
        TransactionError::Storage(error) => {
            EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
        }
    }
}

fn map_embedding_error_to_command_error(error: EmbeddingError) -> CommandError {
    let kind = if error.get_kind() == EmbeddingErrorKind::InternalError {
        CommandErrorKind::InternalError
    } else {
        CommandErrorKind::EmbeddingFailed
    };
    CommandError::new_with_cause(kind, "embedding operation failed", error)
}

fn map_file_error_to_embedding_error(cause: FileError) -> EmbeddingError {
    // Legacy captures classify invalid/stale input separately from failed OS calls.
    // The file boundary retains OS causes even when a syscall reports deletion or NOFOLLOW.
    let kind = match cause.get_kind() {
        FileErrorKind::NonRegular | FileErrorKind::Changed | FileErrorKind::UnsafePath
            if cause.source().is_none() =>
        {
            EmbeddingErrorKind::Input
        }
        _ => EmbeddingErrorKind::InternalError,
    };
    EmbeddingError::new_with_cause(kind, cause)
}

/// Converts an error to an internal embedding error.
///
/// # Arguments
///
/// * `cause` — Failure from an embedding step.
fn map_error_to_embedding_internal_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
}

/// Converts a rejected storage request to an invalid embedding response.
///
/// # Arguments
///
/// * `cause` — Storage failure whose kind selects the embedding classification.
fn map_storage_error_to_embedding_invalid_response_error(cause: StorageError) -> EmbeddingError {
    if cause.get_kind() == StorageErrorKind::InvalidRequest {
        EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidResponse, cause)
    } else {
        EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
    }
}

#[cfg(test)]
#[path = "embedding_test.rs"]
mod tests;
