#[path = "support/cli.rs"]
mod cli;
use cli::{Fixture, finish_foreground};
use letopis::{open_filesystem, service::EmbedLocks};
use std::{fs, os::unix::fs::MetadataExt};

#[test]
fn detached_child_has_exact_context_null_fds_and_no_service_descriptors() {
    // Arrange: capture immutable content before launching through production composition.
    let fixture = Fixture::new();
    let before = fixture.read_preserved_files();
    let mut foreground = fixture.spawn_foreground(None);
    let mut drain = fixture.accept_drain();
    // Act: the foreground must exit and both pipes reach EOF while drain is blocked.
    let facts = drain.read_event();
    drain.send_command("acquire");
    assert_eq!(drain.read_event()["owned"], true);
    let parent = finish_foreground(&mut foreground);
    // Assert: kernel/session/descriptor observations, not fixture claims of completion.
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
    assert_eq!(drain.read_event()["event"], "released");
    drain.expect_eof();
    assert_eq!(fixture.read_preserved_files(), before);
}
#[test]
fn two_released_probes_yield_exactly_one_active_drain() {
    let fixture = Fixture::new();
    let mut first_parent = fixture.spawn_foreground(None);
    let mut first = fixture.accept_drain();
    first.read_event();
    assert!(finish_foreground(&mut first_parent)["warning"].is_null());
    let mut second_parent = fixture.spawn_foreground(None);
    let mut second = fixture.accept_drain();
    second.read_event();
    assert!(finish_foreground(&mut second_parent)["warning"].is_null());
    // Both production probes succeeded before either child may claim ownership.
    first.send_command("acquire");
    assert_eq!(first.read_event()["owned"], true);
    second.send_command("acquire");
    assert_eq!(second.read_event()["owned"], false);
    second.expect_eof();
    first.send_command("release");
    assert_eq!(first.read_event()["event"], "released");
    first.expect_eof();
}
#[test]
fn owned_sigkill_and_stale_pid_preserve_inode_and_release_lock() {
    let fixture = Fixture::new();
    let mut foreground = fixture.spawn_foreground(None);
    let mut drain = fixture.accept_drain();
    drain.read_event();
    finish_foreground(&mut foreground);
    drain.send_command("acquire");
    assert_eq!(drain.read_event()["owned"], true);
    let lock = fixture.root.path().join(".letopis/embed.lock");
    let inode = fs::metadata(&lock).unwrap().ino();
    let files = open_filesystem(fixture.root.path()).unwrap();
    assert!(files.try_lock_embed().unwrap().is_none());
    // Only this owned fixture self-signals; no PID from a file is ever signalled.
    drain.send_command("crash");
    drain.expect_eof();
    let lease = files
        .try_lock_embed()
        .unwrap()
        .expect("SIGKILL released lock");
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    drop(lease);
    fs::write(&lock, "999999999\n").unwrap();
    let lease = files
        .try_lock_embed()
        .unwrap()
        .expect("stale PID is not ownership");
    assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
    drop(lease);
}
#[test]
fn exec_failure_returns_safe_warning() {
    let fixture = Fixture::new();
    let executable = fixture.root.path().join("secret-missing-executable");
    let mut foreground = fixture.spawn_foreground(Some(&executable));
    let response = finish_foreground(&mut foreground);
    assert!(response["warning"]["message"].is_string());
    assert_eq!(response["warning"]["details"], serde_json::json!({}));
    assert!(!response.to_string().contains("secret"));
}
