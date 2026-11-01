use super::{Fixture, LATEST, NEW, Worker, build_service, get_state, get_tokens};
use crate::service::{Refresher, Storage, StorageRead};
use rusqlite::Connection;
use std::{fs, os::unix::process::ExitStatusExt};

#[test]
fn reindex_sigint_preserves_exact_old_or_complete_new_state() {
    for cut in ["locked", "begun", "fts", "before_commit", "after_commit"] {
        // Given: the actual child pauses at an acknowledged operation boundary.
        let fixture = Fixture::new();
        fixture.edit(NEW);
        let expected = fixture.build_expected(NEW, "BravoToken");
        let mut worker = Worker::spawn(fixture.root.path(), "reindex");
        worker.reach(cut);
        // When.
        let pid =
            rustix::process::Pid::from_raw(i32::try_from(worker.child.id()).unwrap()).unwrap();
        rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
        let status = worker.wait_exit();
        // Then: signal death is distinct from TK-29 handled JSON cancellation.
        assert_eq!(status.signal(), Some(2));
        assert_eq!(
            get_state(fixture.root.path()),
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
    // Given: pinned graph and FTS views with a worker paused before commit.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let expected = fixture.build_expected(NEW, "BravoToken");
    let mut storage = build_service(fixture.root.path()).storage;
    let mut pinned = storage.begin_read_tx().unwrap();
    let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
    fts.execute_batch("begin").unwrap();
    assert_eq!(get_tokens(&fts), fixture.old.tokens);
    let mut worker = Worker::spawn(fixture.root.path(), "reindex");
    worker.reach("before_commit");
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    // When: the reindex worker is allowed to commit.
    worker.resume();
    worker.finish();
    // Then: old readers keep the old state and a fresh read sees the complete new state.
    assert_eq!(pinned.get_revision(), 1);
    assert_eq!(pinned.get_nodes().unwrap(), fixture.old.nodes);
    assert_eq!(
        pinned.get_edges_by_source_id(None).unwrap(),
        fixture.old.edges
    );
    assert_eq!(pinned.get_files().unwrap(), fixture.old.files);
    assert_eq!(get_tokens(&fts), fixture.old.tokens);
    assert_eq!(get_state(fixture.root.path()), expected);
}

#[test]
fn publisher_waits_for_reindex_and_redetects_latest_file_bytes() {
    // Given: reindex finishes capturing the old bytes before the next edit.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    let mut owner = Worker::spawn(fixture.root.path(), "reindex");
    owner.reach("before_commit");
    let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
    waiter.reach("contended");
    fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
    let mut expected = fixture.build_expected(LATEST, "CharlieToken");
    expected.revision = 3;
    // When.
    owner.resume();
    owner.finish();
    waiter.resume();
    waiter.finish();
    // Then.
    assert_eq!(get_state(fixture.root.path()), expected);
    fixture.assert_preserved(LATEST);
}

#[test]
fn reindex_late_sql_error_rolls_back_all_state_and_releases_lock() {
    // Given.
    let fixture = Fixture::new();
    fixture.edit(NEW);
    // When.
    Worker::spawn(fixture.root.path(), "reindex-fail").finish();
    // Then.
    assert_eq!(get_state(fixture.root.path()), fixture.old);
    fixture.assert_preserved(NEW);
    Worker::spawn(fixture.root.path(), "reindex").finish();
    assert_eq!(
        get_state(fixture.root.path()),
        fixture.build_expected(NEW, "BravoToken")
    );
}
