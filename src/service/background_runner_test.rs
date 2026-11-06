use super::BackgroundRunnerService;
use crate::service::{
    BackgroundRunner, CommandError, CommandErrorKind, CommandResult, EmbeddingExecutor, FileLocker,
    ProcessError, ProcessLauncher, ProcessResult,
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
    fn is_work_available(&mut self) -> CommandResult<bool> {
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
    is_busy: bool,
    is_failing: bool,
}

impl<'a> FileLocker for Locks<'a> {
    type Lock = Lease<'a>;

    fn lock_acquire_by_path(&self, _: &std::path::Path) -> crate::service::FileResult<Self::Lock> {
        panic!("background must not block")
    }

    fn lock_try_acquire_by_path(
        &self,
        path: &std::path::Path,
    ) -> crate::service::FileResult<Option<Lease<'a>>> {
        assert_eq!(path, std::path::Path::new(".letopis/embed.lock"));
        self.calls.0.borrow_mut().push("probe");
        if self.is_failing {
            return Err(crate::service::FileError::new_with_cause(
                crate::service::FileErrorKind::Io,
                io::Error::other("secret lock payload"),
            ));
        }
        Ok((!self.is_busy).then(|| Lease(self.calls)))
    }
}

struct Process<'a> {
    calls: &'a Calls,
    is_failing: bool,
}

impl ProcessLauncher for Process<'_> {
    fn process_spawn(&self) -> ProcessResult<()> {
        self.calls.0.borrow_mut().push("spawn");
        if self.is_failing {
            return Err(ProcessError::new(io::Error::other(
                "secret executable payload",
            )));
        }
        Ok(())
    }
}

#[test]
fn generated_mocks_do_not_launch_without_work() {
    // Given: an executor reports that no work is ready.
    use crate::service::{MockEmbeddingExecutor, MockProcessLauncher};
    let calls = Calls::default();
    let mut tasks = MockEmbeddingExecutor::new();
    tasks
        .expect_is_work_available()
        .times(1)
        .returning(|| Ok(false));
    let mut process = MockProcessLauncher::new();
    process.expect_process_spawn().times(0);
    let mut service = BackgroundRunnerService::new(Some(&mut tasks), build_locks(&calls), process);

    // When: the background runner checks readiness.
    let result = service.start_if_needed().unwrap();

    // Then: the runner neither probes a lock nor launches a process.
    assert!(result.is_none());
    assert!(calls.0.borrow().is_empty());
}

#[test]
fn absent_capability_does_not_probe_or_spawn() {
    // Given: no embedding executor is configured.
    let calls = Calls::default();
    let mut service =
        BackgroundRunnerService::new(None, build_locks(&calls), build_process(&calls));
    // When / Then: checking readiness produces no warning or background action.
    assert!(service.start_if_needed().unwrap().is_none());
    // Then: no collaborator has been called.
    assert!(calls.0.borrow().is_empty());
    // When / Then: direct drain reports unavailability without calling collaborators.
    assert_eq!(
        service.drain().unwrap_err().get_kind(),
        CommandErrorKind::InternalError
    );
    assert!(calls.0.borrow().is_empty());
}

#[test]
fn ready_work_releases_probe_before_spawn() {
    // Given: work is ready and the lock probe can acquire ownership.
    let calls = Calls::default();
    let mut tasks = build_tasks(&calls, Ok(true));
    let mut service =
        BackgroundRunnerService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
    // When / Then: the runner starts background work without a warning.
    assert!(service.start_if_needed().unwrap().is_none());
    // Then: the probe lease is released before process launch.
    assert_eq!(*calls.0.borrow(), ["ready", "probe", "release", "spawn"]);
}

#[test]
fn busy_probe_does_not_spawn() {
    // Given: work is ready but another caller owns the lock.
    let calls = Calls::default();
    let mut tasks = build_tasks(&calls, Ok(true));
    let locks = Locks {
        is_busy: true,
        ..build_locks(&calls)
    };
    let mut service = BackgroundRunnerService::new(Some(&mut tasks), locks, build_process(&calls));
    // When / Then: the runner leaves background execution to the existing owner.
    assert!(service.start_if_needed().unwrap().is_none());
    // Then: only readiness and the nonblocking probe are called.
    assert_eq!(*calls.0.borrow(), ["ready", "probe"]);
}

#[test]
fn readiness_error_remains_an_error() {
    // Given: the executor cannot determine readiness.
    let calls = Calls::default();
    let error = CommandError::new(CommandErrorKind::EmbeddingFailed, "cannot inspect work");
    let mut tasks = build_tasks(&calls, Err(error));
    let mut service =
        BackgroundRunnerService::new(Some(&mut tasks), build_locks(&calls), build_process(&calls));
    // When / Then: the runner preserves the executor failure classification.
    assert_eq!(
        service.start_if_needed().unwrap_err().get_kind(),
        CommandErrorKind::EmbeddingFailed
    );
    // Then: no lock or launch is attempted after the readiness error.
    assert_eq!(*calls.0.borrow(), ["ready"]);
}

#[test]
fn probe_and_spawn_failures_return_one_safe_warning() {
    // Given: either the lock probe or the subsequent process launch fails.
    for probe_fails in [true, false] {
        // Given: ready work encounters this independent failure case.
        let calls = Calls::default();
        let mut tasks = build_tasks(&calls, Ok(true));
        let locks = Locks {
            is_failing: probe_fails,
            ..build_locks(&calls)
        };
        let process = Process {
            is_failing: !probe_fails,
            ..build_process(&calls)
        };
        let mut service = BackgroundRunnerService::new(Some(&mut tasks), locks, process);
        // When: the runner attempts to start background work.
        let warning = service.start_if_needed().unwrap().unwrap();
        // Then: the exact public warning is safe and later actions stop at the failure.
        assert_eq!(warning.code, CommandErrorKind::InternalError);
        assert_eq!(warning.message, "cannot start background embedding drain");
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
    // Given: an executor is configured for direct drain.
    use crate::service::{MockEmbeddingExecutor, MockProcessLauncher};
    let calls = Calls::default();
    let mut tasks = MockEmbeddingExecutor::new();
    tasks.expect_drain().times(1).returning(|| Ok(()));
    tasks.expect_is_work_available().times(0);
    let mut process = MockProcessLauncher::new();
    process.expect_process_spawn().times(0);
    let mut service = BackgroundRunnerService::new(Some(&mut tasks), build_locks(&calls), process);

    // When: the runner drains background work directly.
    service.drain().unwrap();

    // Then: only the executor runs, without lock probing or launching.
    assert!(calls.0.borrow().is_empty());
}

#[test]
fn direct_drain_preserves_executor_failure() {
    // Given: the configured executor fails during direct drain.
    use crate::service::{MockEmbeddingExecutor, MockProcessLauncher};
    let calls = Calls::default();
    let mut tasks = MockEmbeddingExecutor::new();
    tasks.expect_drain().times(1).returning(|| {
        Err(CommandError::new(
            CommandErrorKind::EmbeddingFailed,
            "drain failed",
        ))
    });
    let mut process = MockProcessLauncher::new();
    process.expect_process_spawn().times(0);
    let mut service = BackgroundRunnerService::new(Some(&mut tasks), build_locks(&calls), process);

    // When: the runner drains background work directly.
    let error = service.drain().unwrap_err();

    // Then: the executor failure remains typed and no process or probe is used.
    assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
    assert!(calls.0.borrow().is_empty());
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
        is_busy: false,
        is_failing: false,
    }
}

fn build_process(calls: &Calls) -> Process<'_> {
    Process {
        calls,
        is_failing: false,
    }
}
