use super::{
    BackgroundRunner, CommandError, CommandErrorKind, CommandProblem, CommandResult,
    EmbeddingExecutor, FileLocker, ProcessLauncher,
};

const BACKGROUND_WARNING: &str = "cannot start background embedding drain";

pub(crate) struct BackgroundRunnerService<'a, F, P> {
    embedding_executor: Option<&'a mut dyn EmbeddingExecutor>,
    file_locker: F,
    process_launcher: P,
}

impl<'a, F: FileLocker, P: ProcessLauncher> BackgroundRunnerService<'a, F, P> {
    pub(crate) fn new(
        embedding_executor: Option<&'a mut dyn EmbeddingExecutor>,
        file_locker: F,
        process_launcher: P,
    ) -> Self {
        Self {
            embedding_executor,
            file_locker,
            process_launcher,
        }
    }
}

impl<F: FileLocker, P: ProcessLauncher> BackgroundRunner for BackgroundRunnerService<'_, F, P> {
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>> {
        let Some(embedding_executor) = self.embedding_executor.as_deref_mut() else {
            return Ok(None);
        };
        if !embedding_executor.is_work_available()? {
            return Ok(None);
        }
        match self
            .file_locker
            .lock_try_acquire_by_path(std::path::Path::new(crate::utils::EMBED_LOCK_PATH))
        {
            Ok(Some(lease)) => drop(lease),
            Ok(None) => return Ok(None),
            Err(_) => {
                return Ok(Some(CommandProblem {
                    code: CommandErrorKind::InternalError,
                    message: BACKGROUND_WARNING,
                    details: Default::default(),
                }));
            }
        }
        Ok(self
            .process_launcher
            .process_spawn()
            .err()
            .map(|_| CommandProblem {
                code: CommandErrorKind::InternalError,
                message: BACKGROUND_WARNING,
                details: Default::default(),
            }))
    }

    fn drain(&mut self) -> CommandResult<()> {
        self.embedding_executor
            .as_deref_mut()
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InternalError,
                    "operation unavailable in this build",
                )
            })?
            .drain()
    }
}

#[cfg(test)]
#[path = "background_runner_test.rs"]
mod tests;
