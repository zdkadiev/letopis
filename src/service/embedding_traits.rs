use super::{
    ConfigState, DrainOutcome, EmbeddingConfig, EmbeddingResult, QueryEmbedding,
    QueryEmbeddingRequest,
};
use crate::domain::RepoPath;
use std::{num::NonZeroUsize, time::Duration};

/// Synchronous texts-to-vectors boundary shared by documents and queries.
///
/// Every implementation validates what it returns and exposes no credentials or response
/// bodies in its errors.
pub trait EmbeddingProvider {
    /// Validate the complete local endpoint/protocol configuration without inference.
    ///
    /// # Errors
    /// Returns invalid configuration when the endpoint or protocol is unsupported.
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;
    /// Compute one attempt and validate every vector before returning it.
    ///
    /// # Errors
    /// Reports invalid config/input, credentials, transport or malformed output.
    fn embed(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>>;
}

/// Confined synchronous file, clock and drain-lock operations.
///
/// Every implementation stays beneath one pinned root and fails rather than returning bytes
/// it observed changing; at most one drain lock is held at a time.
pub trait EmbeddingFiles {
    /// Exclusive drain ownership, released when dropped.
    type EmbedLock;
    /// Read configuration bytes without decoding them; stable absence is `None`.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>>;
    /// Read a current regular document beneath the pinned checkout.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>>;
    /// Try persistent kernel drain ownership without waiting; busy is `None`.
    ///
    /// # Errors
    /// Reports unsafe service paths, observed replacement or I/O failure.
    fn try_lock_embed(&self) -> EmbeddingResult<Option<Self::EmbedLock>>;
    /// Read Unix seconds from the system clock.
    ///
    /// # Errors
    /// Reports a system clock before the Unix epoch.
    fn get_time_secs(&self) -> EmbeddingResult<u64>;
    /// Wait synchronously for a caller-selected retry delay.
    ///
    /// # Errors
    /// Reports an environment failure that prevents the wait; none is currently produced.
    fn wait(&self, delay: Duration) -> EmbeddingResult<()>;
}

/// Synchronous embedding application operations.
///
/// Every implementation keeps document vectors consistent with the committed configuration
/// identity and persists only safe diagnostics.
pub trait Embedder {
    /// Validate current configuration before any vector synchronization. Valid disabled
    /// or unconfigured documents return `ConfigState::Disabled` without observing or
    /// resetting vector identity. A superseded enabled observation returns
    /// `ConfigState::Changed` without retrying or resetting.
    ///
    /// # Errors
    /// Reports storage failures; unreadable or invalid configuration is a typed invalid state.
    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState>;
    /// Drain current eligible documents with bounded concurrent provider attempts.
    /// Each completion is published independently; normal return waits for all
    /// owned requests, including when configuration changes or storage fails.
    ///
    /// # Errors
    /// Reports storage or environment failures that prevent safe coordination.
    ///
    /// # Panics
    /// The fixed worker pool can panic if the OS cannot create its threads.
    /// A worker programming panic is propagated after joining outstanding work.
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome>;
    /// Compute one transient query vector, persisting only a safe diagnostic.
    ///
    /// # Errors
    /// Reports storage or clock failures; ordinary provider errors are unavailable.
    fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding>;
    /// Compute one transient query using immutable captured inputs without rereading or resetting configuration.
    ///
    /// Rejects a changed committed identity and incompatible captured or current dimensions.
    /// No write unit or embedding lock spans inference; document attempts are untouched.
    /// Only the captured identity's guarded safe diagnostic may be persisted or cleared.
    ///
    /// # Errors
    /// Reports storage or clock failures; invalid provider policy and provider failures are unavailable.
    fn embed_query_with_config(
        &mut self,
        request: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding>;
}
