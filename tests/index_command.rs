#[expect(
    dead_code,
    reason = "The race uses one observer from the shared cancellation fixture"
)]
#[path = "support/cancellation.rs"]
mod cancellation;
#[expect(
    dead_code,
    reason = "The race uses the owned child from the shared CLI fixture"
)]
#[path = "support/cli.rs"]
mod cli;
mod support;

use cancellation::{WAIT, observation::observe_index_flock};
use letopis::service::{CacheDatabase, StorageRead};
use letopis::{open_filesystem, service::FileLocker};
use serde_json::{Value, json};
use std::process::{Command, Stdio};
use support::git::isolate_command;
use support::reading::ReadingRepo;

#[test]
fn index_publishes_changes_and_reports_unchanged_corpus() {
    // Given: a repository containing one marked Markdown file and the fixture’s existing live Markdown.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    // When: incremental indexing runs for the first time.
    let output = repo.invoke(&["index"], b"{}");
    // Then: it publishes one file and returns the complete outcome.
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response,
        json!({"ok":true,"result":{"revision":1,"published":true,"files_updated":2,"files_removed":0,"invalid_files_updated":0}})
    );
    // When: the corpus is indexed again without changes.
    let output = repo.invoke(&["index"], b"{}");
    // Then: it reports a no-op without advancing the revision.
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response,
        json!({"ok":true,"result":{"revision":1,"published":false,"files_updated":0,"files_removed":0,"invalid_files_updated":0}})
    );
}

#[test]
fn index_modes_and_scope() {
    // Given: two marked files and an initialized cache.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    repo.write_knowledge("b.md", "BravoToken");
    let initial = repo.invoke(&["index"], b"{}");
    assert!(initial.status.success());
    repo.write_knowledge("a.md", "ChangedToken");
    repo.write_knowledge("b.md", "UntouchedOutsideToken");
    // When: verified mode selects only a.md.
    let output = repo.invoke(&["index", "--verify"], br#"{"paths":["a.md"]}"#);
    // Then: only one file is updated and the other retains its old projection.
    assert!(output.status.success(), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["result"]["files_updated"], 1);
    let storage = repo.open_storage();
    let rows = storage
        .cache
        .with_read_tx(|| storage.cache.text_index_get_by_expression("BravoToken"))
        .unwrap();
    assert_eq!(rows[0].id.as_str(), "b.md");
    // When / Then: both combined flag orders force the selected full reparse.
    for args in [
        ["index", "--full", "--verify"],
        ["index", "--verify", "--full"],
    ] {
        let output = repo.invoke(&args, br#"{"paths":["a.md"]}"#);
        assert!(output.status.success(), "{output:?}");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["result"]["published"], true);
        assert_eq!(response["result"]["files_updated"], 1);
    }
}

#[test]
fn index_rejects_unknown_flags_and_invalid_paths_before_state_creation() {
    // Given: an uninitialized repository.
    let repo = ReadingRepo::new();
    for (args, body) in [
        (vec!["reindex"], "{}"),
        (vec!["index", "--unknown"], "{}"),
        (vec!["index", "--full", "--full"], "{}"),
        (vec!["index"], r#"{"paths":["../escape"]}"#),
        (vec!["index"], r#"{"command":"index"}"#),
    ] {
        // When: the malformed request is invoked.
        let output = repo.invoke(&args, body.as_bytes());
        // Then: it fails with a safe request error and leaves service state absent.
        assert!(!output.status.success());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(!repo.get_root().join(".letopis").exists());
    }
}

#[test]
fn index_reports_edits_invalid_markdown_and_removals() {
    // Given: an indexed marked document.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "OriginalToken");
    assert!(repo.invoke(&["index"], b"{}").status.success());

    // When: its valid content changes.
    repo.write_knowledge("a.md", "ChangedToken");
    let changed = repo.invoke(&["index"], b"{}");
    // Then: exactly one update advances the revision.
    assert!(changed.status.success());
    let response: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(
        (
            response["result"]["revision"].as_u64(),
            response["result"]["files_updated"].as_u64()
        ),
        (Some(2), Some(1))
    );

    // When: malformed markup replaces the selected document.
    repo.write("a.md", b"<!-- letopis: {broken} -->\nInvalidToken\n");
    let invalid = repo.invoke(&["index"], b"{}");
    // Then: it reports the invalid observation while retaining the last valid projection.
    assert!(invalid.status.success());
    let response: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(
        (
            response["result"]["revision"].as_u64(),
            response["result"]["invalid_files_updated"].as_u64()
        ),
        (Some(3), Some(1))
    );
    let storage = repo.open_storage();
    let rows = storage
        .cache
        .with_read_tx(|| storage.cache.text_index_get_by_expression("ChangedToken"))
        .unwrap();
    assert_eq!(rows[0].id.as_str(), "a.md");

    // When: the owner disappears from Git candidate discovery.
    std::fs::remove_file(repo.get_root().join("a.md")).unwrap();
    let removed = repo.invoke(&["index"], b"{}");
    // Then: the removal is counted in the next revision.
    assert!(removed.status.success());
    let response: Value = serde_json::from_slice(&removed.stdout).unwrap();
    assert_eq!(
        (
            response["result"]["revision"].as_u64(),
            response["result"]["files_removed"].as_u64()
        ),
        (Some(4), Some(1))
    );
}

#[test]
fn two_scoped_publishers_redetect_after_index_lock_contention() {
    // Given: two independent product processes wait on the same held index lock.
    let repo = ReadingRepo::new();
    repo.write_knowledge("selected.md", "OriginalToken");
    repo.write_knowledge("outside.md", "OutsideToken");
    assert!(repo.invoke(&["index"], b"{}").status.success());
    for (mode, body) in [
        (&["index"][..], "FirstEditToken"),
        (&["index", "--verify"][..], "SecondEditToken"),
    ] {
        repo.write_knowledge("selected.md", body);
        let filesystem = open_filesystem(repo.get_root()).unwrap();
        let holder = filesystem
            .lock_acquire_by_path(std::path::Path::new(".letopis/index.lock"))
            .unwrap();
        let lock = repo.get_root().join(".letopis/index.lock");
        let mut children = Vec::new();
        for _ in 0..2 {
            children.push(spawn_scoped_index_waiter(&repo, mode, &lock));
        }

        // When: the holder releases the lock after both processes are waiting.
        drop(holder);
        let responses: Vec<Value> = children
            .iter_mut()
            .map(|child| {
                let output = child.collect_output(WAIT);
                assert!(output.status.success(), "{output:?}");
                serde_json::from_slice(&output.stdout).unwrap()
            })
            .collect();

        // Then: one publication wins and the other observes its committed revision.
        let mut counts: Vec<_> = responses
            .iter()
            .map(|response| {
                (
                    response["result"]["published"].as_bool().unwrap(),
                    response["result"]["files_updated"].as_u64().unwrap(),
                )
            })
            .collect();
        counts.sort();
        assert_eq!(counts, [(false, 0), (true, 1)]);
        let revision = if mode.len() == 1 { 2 } else { 3 };
        assert!(
            responses
                .iter()
                .all(|response| response["result"]["revision"] == revision)
        );
        let storage = repo.open_storage();
        let outside = storage
            .cache
            .with_read_tx(|| storage.cache.text_index_get_by_expression("OutsideToken"))
            .unwrap();
        assert_eq!(outside[0].id.as_str(), "outside.md");
    }
}

fn spawn_scoped_index_waiter(
    repo: &ReadingRepo,
    mode: &[&str],
    lock: &std::path::Path,
) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    process
        .current_dir(repo.get_root())
        .args(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cli::OwnedChild::spawn(&mut process);
    child.send_input(br#"{"paths":["selected.md"]}"#);
    let observation = observe_index_flock(child.get_id(), lock);
    assert!(observation.starts_with("inside=flock"), "{observation}");
    child
}
