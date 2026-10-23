use super::{
    BackgroundRunner, CommandError, CommandErrorKind, CommandProblem, CommandResult, DrainLauncher,
    EmbedLocker, EmbeddingExecutor,
};

pub(crate) struct BackgroundService<'a, F, P> {
    tasks: Option<&'a mut dyn EmbeddingExecutor>,
    files: F,
    process: P,
}
impl<'a, F: EmbedLocker, P: DrainLauncher> BackgroundService<'a, F, P> {
    pub(crate) fn new(tasks: Option<&'a mut dyn EmbeddingExecutor>, files: F, process: P) -> Self {
        Self {
            tasks,
            files,
            process,
        }
    }
}
impl<F: EmbedLocker, P: DrainLauncher> BackgroundRunner for BackgroundService<'_, F, P> {
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>> {
        let Some(tasks) = self.tasks.as_deref_mut() else {
            return Ok(None);
        };
        if !tasks.has_work()? {
            return Ok(None);
        }
        match self.files.try_lock_embed() {
            Ok(Some(lease)) => drop(lease),
            Ok(None) => return Ok(None),
            Err(_) => return Ok(Some(build_warning())),
        }
        Ok(self.process.spawn_drain().err().map(|_| build_warning()))
    }
    fn drain(&mut self) -> CommandResult<()> {
        self.tasks
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
fn build_warning() -> CommandProblem {
    CommandProblem {
        code: CommandErrorKind::InternalError,
        message: "cannot start background embedding drain",
        details: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::BackgroundService;
    use crate::service::{
        BackgroundRunner, CommandError, CommandErrorKind, CommandResult, DocumentError,
        DocumentErrorKind, DocumentResult, DrainLauncher, EmbedLocker, EmbeddingExecutor,
    };
    use std::{cell::RefCell, io};

    #[derive(Default)]
    struct Calls(RefCell<Vec<&'static str>>);
    struct Tasks<'a> {
        calls: &'a Calls,
        ready: CommandResult<bool>,
        result: CommandResult<()>,
    }
    impl EmbeddingExecutor for Tasks<'_> {
        fn has_work(&mut self) -> CommandResult<bool> {
            self.calls.0.borrow_mut().push("ready");
            std::mem::replace(&mut self.ready, Ok(false))
        }
        fn drain(&mut self) -> CommandResult<()> {
            self.calls.0.borrow_mut().push("drain");
            std::mem::replace(&mut self.result, Ok(()))
        }
    }
    struct Lease<'a>(&'a Calls);
    impl Drop for Lease<'_> {
        fn drop(&mut self) {
            self.0.0.borrow_mut().push("release");
        }
    }
    struct Locks<'a> {
        calls: &'a Calls,
        busy: bool,
        fails: bool,
    }
    impl<'a> EmbedLocker for Locks<'a> {
        type EmbedLock = Lease<'a>;
        fn try_lock_embed(&self) -> DocumentResult<Option<Lease<'a>>> {
            self.calls.0.borrow_mut().push("probe");
            if self.fails {
                return Err(DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "lock failed",
                    io::Error::other("secret lock payload"),
                ));
            }
            Ok((!self.busy).then(|| Lease(self.calls)))
        }
    }
    struct Process<'a> {
        calls: &'a Calls,
        fails: bool,
    }
    impl DrainLauncher for Process<'_> {
        fn spawn_drain(&self) -> CommandResult<()> {
            self.calls.0.borrow_mut().push("spawn");
            if self.fails {
                return Err(CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "spawn failed",
                    io::Error::other("secret executable payload"),
                ));
            }
            Ok(())
        }
    }
    #[test]
    fn absent_capability_does_not_probe_or_spawn() {
        let calls = Calls::default();
        let mut service = BackgroundService::new(None, build_locks(&calls), build_process(&calls));
        assert!(service.start_if_needed().unwrap().is_none());
        assert!(calls.0.borrow().is_empty());
        assert_eq!(
            service.drain().unwrap_err().get_kind(),
            CommandErrorKind::InternalError
        );
        assert!(calls.0.borrow().is_empty());
    }
    #[test]
    fn no_work_does_not_probe_or_spawn() {
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(false));
        let mut service =
            BackgroundService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
        assert!(service.start_if_needed().unwrap().is_none());
        assert_eq!(*calls.0.borrow(), ["ready"]);
    }
    #[test]
    fn ready_work_releases_probe_before_spawn() {
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(true));
        let mut service =
            BackgroundService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
        assert!(service.start_if_needed().unwrap().is_none());
        assert_eq!(*calls.0.borrow(), ["ready", "probe", "release", "spawn"]);
    }
    #[test]
    fn busy_probe_does_not_spawn() {
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(true));
        let locks = Locks {
            busy: true,
            ..build_locks(&calls)
        };
        let mut service = BackgroundService::new(Some(&mut tasks), locks, build_process(&calls));
        assert!(service.start_if_needed().unwrap().is_none());
        assert_eq!(*calls.0.borrow(), ["ready", "probe"]);
    }
    #[test]
    fn readiness_error_remains_an_error() {
        let calls = Calls::default();
        let error = CommandError::new(CommandErrorKind::EmbeddingFailed, "cannot inspect work");
        let mut tasks = build_tasks(&calls, Err(error));
        let mut service =
            BackgroundService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
        assert_eq!(
            service.start_if_needed().unwrap_err().get_kind(),
            CommandErrorKind::EmbeddingFailed
        );
        assert_eq!(*calls.0.borrow(), ["ready"]);
    }
    #[test]
    fn probe_and_spawn_failures_return_one_safe_warning() {
        for probe_fails in [true, false] {
            let calls = Calls::default();
            let mut tasks = build_tasks(&calls, Ok(true));
            let locks = Locks {
                fails: probe_fails,
                ..build_locks(&calls)
            };
            let process = Process {
                fails: !probe_fails,
                ..build_process(&calls)
            };
            let mut service = BackgroundService::new(Some(&mut tasks), locks, process);
            let warning = service.start_if_needed().unwrap().unwrap();
            assert_eq!(warning.code, CommandErrorKind::InternalError);
            assert!(warning.details.is_empty());
            assert!(!format!("{warning:?}").contains("secret"));
            let expected = if probe_fails {
                vec!["ready", "probe"]
            } else {
                vec!["ready", "probe", "release", "spawn"]
            };
            assert_eq!(*calls.0.borrow(), expected);
        }
    }
    #[test]
    fn direct_drain_calls_executor_once_without_second_lock() {
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(true));
        let mut service =
            BackgroundService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
        service.drain().unwrap();
        assert_eq!(*calls.0.borrow(), ["drain"]);
    }
    #[test]
    fn direct_drain_preserves_executor_failure() {
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(true));
        tasks.result = Err(CommandError::new(
            CommandErrorKind::EmbeddingFailed,
            "drain failed",
        ));
        let mut service =
            BackgroundService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
        assert_eq!(
            service.drain().unwrap_err().get_kind(),
            CommandErrorKind::EmbeddingFailed
        );
        assert_eq!(*calls.0.borrow(), ["drain"]);
    }

    fn build_tasks(calls: &Calls, ready: CommandResult<bool>) -> Tasks<'_> {
        Tasks {
            calls,
            ready,
            result: Ok(()),
        }
    }

    fn build_locks(calls: &Calls) -> Locks<'_> {
        Locks {
            calls,
            busy: false,
            fails: false,
        }
    }

    fn build_process(calls: &Calls) -> Process<'_> {
        Process {
            calls,
            fails: false,
        }
    }
}
