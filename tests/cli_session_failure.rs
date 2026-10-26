//! Isolated syscall-failure proof; the interposed symbol exists only in this test executable.
#![cfg(any(target_os = "macos", target_os = "linux"))]
use letopis::{
    service::{BackgroundRunner, CommandErrorKind, CommandResult, EmbeddingExecutor},
    wire_background_runner,
};
use std::path::Path;

mockall::mock! {
    ReadyWork {}
    impl EmbeddingExecutor for ReadyWork {
        fn has_work(&mut self) -> CommandResult<bool>;
        fn drain(&mut self) -> CommandResult<()>;
    }
}

#[test]
fn reports_a_safe_warning_when_the_child_cannot_create_its_session() {
    // Given: the executable exists, so failed exec cannot mask a lost setsid error.
    let root = tempfile::tempdir().unwrap();
    let executable = Path::new("/usr/bin/true");
    assert!(executable.is_file());
    let mut tasks = MockReadyWork::new();
    tasks.expect_has_work().times(1).returning(|| Ok(true));
    tasks.expect_drain().times(0);
    let mut background = wire_background_runner(root.path(), executable, Some(&mut tasks)).unwrap();
    // When
    let warning = background
        .start_if_needed()
        .unwrap()
        .expect("session setup must fail");
    // Then
    assert_eq!(warning.code, CommandErrorKind::InternalError);
    assert!(warning.details.is_empty());
    let root_text = root.path().display().to_string();
    assert!(!warning.message.contains(root_text.as_str()));
}

// The executable-local definition intercepts the real libc boundary called by
// DetachedProcess's pre_exec closure. It must remain async-signal-safe after fork.
#[unsafe(export_name = "setsid")]
extern "C" fn fail_session_creation() -> libc::pid_t {
    // SAFETY: each supported platform returns the calling thread's valid errno
    // pointer. Writing EPERM neither allocates nor acquires a userspace lock.
    unsafe {
        #[cfg(target_os = "macos")]
        {
            *libc::__error() = libc::EPERM;
        }
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location() = libc::EPERM;
        }
    }
    -1
}
