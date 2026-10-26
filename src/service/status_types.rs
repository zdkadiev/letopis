use super::{CommandProblem, EmbeddingFailure};
use crate::domain::Sha256;
use std::num::NonZeroU32;

/// Current observations; the database fields share one published read view.
#[derive(Debug)]
pub struct StatusReport {
    /// Published revision, or no cache publication.
    pub revision: Option<u64>,
    /// Distinct current knowledge hashes awaiting a usable vector.
    pub embedding_backlog: u64,
    /// Distinct current knowledge hashes with exhausted attempts and no vector.
    pub embedding_failed: u64,
    /// Best-effort recorded PIDs for observed-busy kernel locks.
    pub locks: Vec<HeldLock>,
    /// Deterministically ordered current safe diagnostics.
    pub problems: Vec<CommandProblem>,
}

/// Persistent kernel-lock responsibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockName {
    /// Cache publication exclusion.
    Index,
    /// Embedding drain exclusion.
    Embed,
}

/// A busy lock and its positive recorded PID; ownership is not atomic with PID reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldLock {
    /// Observed lock.
    pub name: LockName,
    /// Recorded positive PID, which may briefly belong to a previous owner.
    pub pid: NonZeroU32,
}

/// Captured configuration policy; observing it never resets stored identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingStatusPolicy {
    /// Every current hash is backlog and none has current exhausted attempts.
    Unavailable,
    /// Use stored coverage and attempts only when the pinned identity matches.
    Ready {
        /// Captured valid configuration identity.
        identity: Sha256,
        /// Positive total durable attempt budget, including the initial attempt.
        max_attempts: u64,
    },
}

/// Distinct current knowledge hash counts and their exhausted reservations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingStatusSummary {
    /// Current hashes without usable vectors or exhausted reservations.
    pub backlog: u64,
    /// Current hashes with exhausted reservations and no usable vectors.
    pub failed: u64,
    /// Exhausted current hashes, once each in ascending hash order.
    pub exhausted: Vec<EmbeddingFailure>,
}
