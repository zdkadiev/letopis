// Shared process helpers are also consumed by the real-drain integration target.
#[allow(dead_code)]
#[path = "support/cli.rs"]
mod cli;
use cli::{Fixture, finish_foreground, wait_for_embed_lock};
use letopis::{open_filesystem, service::FileLocker};
use std::{cell::RefCell, fs, os::unix::fs::MetadataExt};

#[test]
fn detached_child_has_exact_context_null_fds_and_no_service_descriptors() {
    // Given: capture immutable content before launching through production composition.
    let fixture = Fixture::new();
    let before = fixture.get_preserved_files();
    let mut foreground = fixture.spawn_foreground(None);
    let mut drain = fixture.accept_drain();
    // When: the foreground must exit and both pipes reach EOF while drain is blocked.
    let facts = drain.get_event();
    drain.send_command("acquire");
    assert_eq!(drain.get_event()["owned"], true);
    let parent = finish_foreground(&mut foreground);
    // Then: kernel/session/descriptor observations, not fixture claims of completion.
    assert_eq!(facts["argv"], serde_json::json!(["embed", "--drain"]));
    assert_eq!(
        facts["cwd"],
        fs::canonicalize(fixture.root.path())
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(facts["sentinel"], "safe inherited sentinel");
    assert_eq!(
        facts["null_fds"],
        serde_json::json!([true, true, true]),
        "{facts}"
    );
    assert_eq!(facts["inherited"], serde_json::json!([]));
    assert_eq!(facts["sid"], facts["pid"]);
    assert_ne!(facts["sid"], parent["sid"]);
    assert!(parent["warning"].is_null());
    drain.send_command("release");
    assert_eq!(drain.get_event()["event"], "released");
    drain.expect_eof();
    assert_eq!(fixture.get_preserved_files(), before);
}
#[test]
fn two_released_probes_yield_exactly_one_active_drain() {
    // Given
    let fixture = Fixture::new();
    let mut first_parent = fixture.spawn_foreground(None);
    // When
    let mut first = fixture.accept_drain();
    first.get_event();
    // Then
    assert!(finish_foreground(&mut first_parent)["warning"].is_null());
    let mut second_parent = fixture.spawn_foreground(None);
    let mut second = fixture.accept_drain();
    second.get_event();
    assert!(finish_foreground(&mut second_parent)["warning"].is_null());
    // Both production probes succeeded before either child may claim ownership.
    first.send_command("acquire");
    assert_eq!(first.get_event()["owned"], true);
    second.send_command("acquire");
    assert_eq!(second.get_event()["owned"], false);
    second.expect_eof();
    first.send_command("release");
    assert_eq!(first.get_event()["event"], "released");
    first.expect_eof();
}
#[test]
fn owned_sigkill_and_stale_pid_preserve_inode_and_release_lock() {
    // Given: a process owns the persistent lock and has recorded its PID.
    let fixture = Fixture::new();
    let mut foreground = fixture.spawn_foreground(None);
    let mut drain = fixture.accept_drain();
    drain.get_event();
    finish_foreground(&mut foreground);
    drain.send_command("acquire");
    // When / Then: the owner is killed and another caller probes; kernel ownership releases while the inode remains persistent.
    assert_eq!(drain.get_event()["owned"], true);
    let lock = fixture.root.path().join(".letopis/embed.lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    let files = open_filesystem(fixture.root.path()).unwrap();
    assert!(
        files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    // Only this owned fixture self-signals; no PID from a file is ever signalled.
    drain.send_command("crash");
    drain.expect_eof();
    let lease = wait_for_embed_lock(&files);
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    drop(lease);
    fs::write(&lock, "999999999\n").unwrap();
    let lease = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .expect("stale PID is not ownership");
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    drop(lease);
}
#[test]
fn exec_failure_returns_safe_warning() {
    // Given
    let fixture = Fixture::new();
    let executable = fixture.root.path().join("secret-missing-executable");
    let mut foreground = fixture.spawn_foreground(Some(&executable));
    // When
    let response = finish_foreground(&mut foreground);
    // Then
    assert!(response["warning"]["message"].is_string());
    assert_eq!(response["warning"]["details"], serde_json::json!({}));
    assert!(!response.to_string().contains("secret"));
}

struct ReleaseAfterBusyProbe<F: FileLocker> {
    files: F,
    owner: RefCell<Option<F::Lock>>,
}
impl<F: FileLocker> FileLocker for ReleaseAfterBusyProbe<F> {
    type Lock = F::Lock;
    fn lock_acquire_by_path(
        &self,
        path: &std::path::Path,
    ) -> letopis::service::FileResult<Self::Lock> {
        self.files.lock_acquire_by_path(path)
    }

    fn lock_try_acquire_by_path(
        &self,
        path: &std::path::Path,
    ) -> letopis::service::FileResult<Option<Self::Lock>> {
        let result = self.files.lock_try_acquire_by_path(path)?;
        if result.is_none() {
            // Deterministically model cleanup after an observer saw an earlier
            // completion event: this first real probe must still report busy.
            self.owner.borrow_mut().take();
        }
        Ok(result)
    }
}

#[test]
fn lock_release_wait_retries_busy_probe_and_retains_acquired_lease() {
    // Given: the observer initially sees a genuinely occupied kernel lock.
    let root = tempfile::tempdir().unwrap();
    let files = open_filesystem(root.path()).unwrap();
    let owner = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let lock = root.path().join(".letopis/embed.lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    let delayed = ReleaseAfterBusyProbe {
        files,
        owner: RefCell::new(Some(owner)),
    };

    // When: the first failed probe permits the independent owner to release.
    let lease = wait_for_embed_lock(&delayed);

    // Then: completion requires an actual lease, retained through observation.
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    assert!(
        delayed
            .files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    drop(lease);
    assert!(
        delayed
            .files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_some()
    );
}
