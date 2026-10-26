use super::{Fixture, LATEST, NEW, Worker, build_service, read_state, read_tokens};
use crate::service::{Refresher, Storage, StorageRead};
use rusqlite::Connection;
use std::{fs, os::unix::process::ExitStatusExt};

#[test]
fn reindex_sigint_preserves_exact_old_or_complete_new_state() {
    for cut in ["locked", "begun", "fts", "before_commit", "after_commit"] {
        // Arrange: the actual child pauses at an acknowledged operation boundary.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let mut worker = Worker::spawn(fixture.root.path(), "reindex");
        worker.reach(cut);
        // Act.
        let pid =
            rustix::process::Pid::from_raw(i32::try_from(worker.child.id()).unwrap()).unwrap();
        rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
        let status = worker.wait_exit();
        // Assert: signal death is distinct from TK-29 handled JSON cancellation.
        assert_eq!(status.signal(), Some(2));
        assert_eq!(
            read_state(fixture.root.path()),
            if cut == "after_commit" {
                expected
            } else {
                fixture.old.clone()
            }
        );
        fixture.assert_preserved(NEW);
        // The process's kernel lease has been released; a fresh call can publish.
        let mut service = build_service(fixture.root.path());
        assert!(service.reindex().unwrap().published);
    }
}

#[test]
fn reindex_keeps_pinned_wal_view_and_publishes_one_complete_new_view() {
    // Arrange.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let expected = fixture.build_expected(NEW, "BravoToken");
    let mut storage = build_service(fixture.root.path()).storage;
    let mut pinned = storage.begin_read().unwrap();
    let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
    fts.execute_batch("BEGIN").unwrap();
    assert_eq!(read_tokens(&fts), fixture.old.tokens);
    let mut worker = Worker::spawn(fixture.root.path(), "reindex");
    worker.reach("before_commit");
    assert_eq!(read_state(fixture.root.path()), fixture.old);
    // Act.
    worker.resume();
    worker.finish();
    // Assert.
    assert_eq!(pinned.get_revision(), 1);
    assert_eq!(pinned.list_nodes().unwrap(), fixture.old.nodes);
    assert_eq!(pinned.list_edges(None).unwrap(), fixture.old.edges);
    assert_eq!(pinned.list_files().unwrap(), fixture.old.files);
    assert_eq!(read_tokens(&fts), fixture.old.tokens);
    assert_eq!(read_state(fixture.root.path()), expected);
}

#[test]
fn publisher_waits_for_reindex_and_redetects_latest_file_bytes() {
    // Arrange: reindex finishes capturing the old bytes before the next edit.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let mut owner = Worker::spawn(fixture.root.path(), "reindex");
    owner.reach("before_commit");
    let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
    waiter.reach("contended");
    fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
    let mut expected = fixture.build_expected(LATEST, "CharlieToken");
    expected.revision = 3;
    // Act.
    owner.resume();
    owner.finish();
    waiter.resume();
    waiter.finish();
    // Assert.
    assert_eq!(read_state(fixture.root.path()), expected);
    fixture.assert_preserved(LATEST);
}

#[test]
fn reindex_late_sql_error_rolls_back_all_state_and_releases_lock() {
    // Arrange.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    // Act.
    Worker::spawn(fixture.root.path(), "reindex-fail").finish();
    // Assert.
    assert_eq!(read_state(fixture.root.path()), fixture.old);
    fixture.assert_preserved(NEW);
    Worker::spawn(fixture.root.path(), "reindex").finish();
    assert_eq!(
        read_state(fixture.root.path()),
        fixture.build_expected(NEW, "BravoToken")
    );
}
