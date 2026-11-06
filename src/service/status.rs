use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandProblem, CommandResult, Configuration,
    EmbeddingFailure, EmbeddingProvider, FileReader, GraphDiagnostics, SourceProvenance,
    StatusReader, StatusReport, StorageError, StorageErrorKind, StorageResult,
    utils::{CheckObservations, REASON, TARGET, sort_problems},
};
use super::{HeldLock, LockName};
use crate::domain::Sha256;
use crate::service::FileResult;
use crate::utils::{CONFIG_PATH, SERVICE_DIRECTORY};
use crate::utils::{EMBED_LOCK_PATH, INDEX_LOCK_PATH};
use serde_json::Value;
use std::path::Path;
use std::{collections::BTreeMap, fmt, rc::Rc};

const INVALID_CONFIG: &str = "invalid_embedding_config";
const QUERY_FAILURE: &str = "query_embedding_failed";
const EXHAUSTED: &str = "embedding_attempts_exhausted";

/// Captured configuration policy; observing it never resets stored identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingStatusPolicy {
    /// Every current hash is backlog and none has current exhausted attempts.
    Unavailable,
    /// Valid disabled configuration: report no embedding work or execution failures.
    Disabled,
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
struct EmbeddingStatusSummary {
    /// Current hashes without usable vectors or exhausted reservations.
    backlog: u64,
    /// Current hashes with exhausted reservations and no usable vectors.
    failed: u64,
    /// Exhausted current hashes, once each in ascending hash order.
    exhausted: Vec<EmbeddingFailure>,
}

/// Observes the last publication and live coordination without changing either.
pub(crate) struct StatusService<'a, C> {
    pub(crate) cache: Rc<C>,
    pub(crate) file_reader: &'a dyn FileReader,
    pub(crate) graph: &'a dyn GraphDiagnostics,
    pub(crate) provenance: &'a dyn SourceProvenance,
    pub(crate) provider: &'a dyn EmbeddingProvider,
}

impl<C> fmt::Debug for StatusService<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusService").finish_non_exhaustive()
    }
}

impl<C: CacheDatabase> StatusReader for StatusService<'_, C> {
    fn get_status(&mut self) -> CommandResult<StatusReport> {
        let (policy, config_problem) = self.get_config_policy();
        let published = self
            .cache
            .with_status_read_tx(|| {
                let observations = CheckObservations {
                    storage_read: self.cache.as_ref(),
                    graph: self.graph,
                    provenance: self.provenance,
                };
                let problems = observations.collect_check_problems(&[])?;
                self.get_published(&policy, problems)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot read published status",
                    error,
                ),
            })?;
        let mut report = published.unwrap_or(StatusReport {
            revision: None,
            embedding_backlog: 0,
            embedding_failed: 0,
            locks: Vec::new(),
            problems: Vec::new(),
        });
        if let Some(problem) = config_problem {
            report.problems.push(problem);
        }
        report.locks = self.get_locks()?;
        report.problems = sort_problems(report.problems);
        Ok(report)
    }
}

impl<C: CacheDatabase> StatusService<'_, C> {
    fn get_config_policy(&self) -> (EmbeddingStatusPolicy, Option<CommandProblem>) {
        let Ok(bytes) = self.get_config_bytes() else {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        };
        let Some(bytes) = bytes else {
            return (EmbeddingStatusPolicy::Unavailable, None);
        };
        let Ok(configuration) = Configuration::parse(&bytes) else {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        };
        if let Some(config) = configuration.get_embedding_config()
            && self.provider.config_validate(config).is_err()
        {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        }
        if !configuration.is_enabled() {
            return (EmbeddingStatusPolicy::Disabled, None);
        }
        let Some(config) = configuration.get_embedding_config() else {
            return (EmbeddingStatusPolicy::Disabled, None);
        };
        (
            EmbeddingStatusPolicy::Ready {
                identity: config.get_identity(),
                max_attempts: config.get_max_attempts(),
            },
            None,
        )
    }

    fn get_config_bytes(&self) -> FileResult<Option<Vec<u8>>> {
        self.file_reader
            .directory_get_by_path(Path::new(SERVICE_DIRECTORY))?;
        self.file_reader
            .file_get_bytes_by_path(Path::new(CONFIG_PATH))
    }

    fn get_published(
        &self,
        policy: &EmbeddingStatusPolicy,
        mut problems: Vec<CommandProblem>,
    ) -> CommandResult<StatusReport> {
        let revision = self.cache.project_get_by_id().map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot read published status",
                cause,
            )
        })?;
        let summary = self.get_embedding_status(policy).map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot read published status",
                cause,
            )
        })?;
        if !matches!(policy, EmbeddingStatusPolicy::Disabled) {
            problems.extend(collect_exhausted(&summary));
            if self
                .cache
                .query_error_get_by_id()
                .map_err(|cause| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot read published status",
                        cause,
                    )
                })?
                .is_some()
            {
                problems.push(build_problem(
                    CommandErrorKind::EmbeddingFailed,
                    "query embedding failed",
                    QUERY_FAILURE,
                    None,
                ));
            }
        }
        Ok(StatusReport {
            revision: Some(revision),
            embedding_backlog: summary.backlog,
            embedding_failed: summary.failed,
            locks: Vec::new(),
            problems,
        })
    }

    fn get_embedding_status(
        &self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        if matches!(policy, EmbeddingStatusPolicy::Disabled) {
            return Ok(EmbeddingStatusSummary {
                backlog: 0,
                failed: 0,
                exhausted: vec![],
            });
        }
        let total = self.cache.nodes_get_by_kind()?;
        let EmbeddingStatusPolicy::Ready {
            identity,
            max_attempts,
        } = policy
        else {
            return Ok(EmbeddingStatusSummary {
                backlog: total,
                failed: 0,
                exhausted: vec![],
            });
        };
        if *max_attempts == 0 || i64::try_from(*max_attempts).is_err() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid status attempt budget",
            ));
        }
        if self.cache.meta_get_by_id()? != Some(*identity) {
            return Ok(EmbeddingStatusSummary {
                backlog: total,
                failed: 0,
                exhausted: vec![],
            });
        }
        self.get_matching_embedding_status(total, *max_attempts)
    }

    fn get_matching_embedding_status(
        &self,
        total: u64,
        max_attempts: u64,
    ) -> StorageResult<EmbeddingStatusSummary> {
        let covered = self.cache.nodes_get_coverage_by_kind()?;
        let exhausted = self
            .cache
            .embed_failures_get_by_kind_and_search_hash_and_attempts(max_attempts)?;
        let failed = u64::try_from(exhausted.len()).map_err(|cause| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                cause,
            )
        })?;
        let backlog = total
            .checked_sub(covered)
            .and_then(|missing| missing.checked_sub(failed))
            .ok_or_else(|| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid embedding status counts",
                )
            })?;
        Ok(EmbeddingStatusSummary {
            backlog,
            failed,
            exhausted,
        })
    }

    fn get_locks(&self) -> CommandResult<Vec<HeldLock>> {
        let mut locks = Vec::new();
        for (path, name) in [
            (INDEX_LOCK_PATH, LockName::Index),
            (EMBED_LOCK_PATH, LockName::Embed),
        ] {
            let owner = self
                .file_reader
                .lock_get_owner_by_path(Path::new(path))
                .map_err(|cause| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot observe coordination lock owner",
                        cause,
                    )
                })?;
            if let Some(pid) = owner {
                locks.push(HeldLock { name, pid });
            }
        }
        Ok(locks)
    }
}

fn collect_exhausted(summary: &EmbeddingStatusSummary) -> Vec<CommandProblem> {
    summary
        .exhausted
        .iter()
        .map(|failure| {
            build_problem(
                CommandErrorKind::EmbeddingFailed,
                "embedding attempts exhausted",
                EXHAUSTED,
                Some(&failure.search_hash.to_hex()),
            )
        })
        .collect()
}

fn invalid_config_problem() -> CommandProblem {
    build_problem(
        CommandErrorKind::ValidationFailed,
        "invalid embedding configuration",
        INVALID_CONFIG,
        None,
    )
}

fn build_problem(
    code: CommandErrorKind,
    message: &'static str,
    reason: &'static str,
    target: Option<&str>,
) -> CommandProblem {
    let mut details = BTreeMap::from([(REASON.into(), Value::from(reason))]);
    if let Some(target) = target {
        details.insert(TARGET.into(), Value::from(target));
    }
    CommandProblem {
        code,
        message,
        details,
    }
}

#[cfg(test)]
#[path = "status_test.rs"]
mod tests;
