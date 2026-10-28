#[expect(
    dead_code,
    reason = "Shared owned-process fixture exposes unrelated CLI runtime helpers"
)]
#[path = "support/cli.rs"]
mod cli;
mod support;

#[path = "support/cancellation.rs"]
mod cancellation;

use cancellation::observation::{observe_index_flock, observe_stdin_wait};
use cancellation::{
    CANCELLED_ENVELOPE, GitProxy, ObserverSocket, WAIT, WrapperConnection, assert_cancelled_output,
    assert_index_lock_held, create_stdin_fifo, read_lock_identity, wait_until_index_lock_free,
};
use letopis::{open_filesystem, service::FileSystem};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io::Write,
    process::{Command, Stdio},
};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    git::isolate_command,
    reading::ReadingRepo,
};

const CANCELLATION_CANARY: &str = "TK29_CANCELLATION_CANARY";
const STDIN_PREFIX: &str = r#"{"canary":"TK29_CANCELLATION_CANARY"#;
const FOREGROUND_COMMANDS: [&str; 5] = ["search", "traverse", "inspect", "check", "reindex"];

#[test]
fn cancels_each_foreground_command_while_reading_stdin() {
    // Given: the shipped binary reads a partial request from a live test FIFO.
    let repo = ReadingRepo::new();
    for command in FOREGROUND_COMMANDS {
        let fifo = tempfile::tempdir().expect("FIFO directory");
        let (mut stdin, address) = create_stdin_fifo(fifo.path());
        let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
        isolate_command(&mut process);
        process
            .current_dir(repo.get_root())
            .arg(command)
            .stdin(Stdio::from(stdin.try_clone().expect("stdin clone")))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cli::OwnedChild::spawn(&mut process);
        stdin
            .write_all(STDIN_PREFIX.as_bytes())
            .expect("partial request");
        // When: cancellation arrives only after the native stdin wait is observed.
        let observation = observe_stdin_wait(child.get_id(), &address);
        assert!(observation.starts_with("inside=stdin"), "{observation}");
        child.send_sigint();
        let output = child.collect_output(WAIT);
        // Then: exactly one cancelled envelope, exit 1 and no request text.
        assert_cancelled_output(&output, command, &[CANCELLATION_CANARY, "canary"]);
        assert_eq!(output.stdout, CANCELLED_ENVELOPE);
    }
}

#[test]
fn cancels_waiting_index_lock_without_killing_holder() {
    // Given: this test owns a real kernel index lock and therefore its recorded PID.
    let repo = ReadingRepo::new();
    let filesystem = open_filesystem(repo.get_root()).expect("filesystem");
    let holder = filesystem.lock_index().expect("holder lock");
    let lock = repo.get_root().join(".letopis/index.lock");
    let identity = read_lock_identity(&lock);
    let mut child = spawn_reading(&repo, "reindex");
    child.send_input(b"{}");
    // When: signal only after the product main thread is blocked in flock.
    let observation = observe_index_flock(child.get_id(), &lock);
    assert!(observation.starts_with("inside=flock"), "{observation}");
    child.send_sigint();
    let output = child.collect_output(WAIT);
    // Then: the holder is intact and its lock file is unchanged.
    assert_cancelled_output(&output, "lock waiter", &[CANCELLATION_CANARY]);
    assert_index_lock_held(&lock);
    assert_eq!(read_lock_identity(&lock), identity);
    drop(holder);
    let completed = repo.invoke(&["reindex"], b"{}");
    assert_eq!(completed.status.code(), Some(0), "{completed:?}");
    let response: Value = serde_json::from_slice(&completed.stdout).expect("reindex response");
    assert_eq!(response["ok"], true);
}

#[test]
fn cancels_while_git_child_is_running() {
    // Given: a test proxy reports the live ls-files child while the product holds index.lock.
    let repo = ReadingRepo::new();
    let observer = ObserverSocket::new();
    let proxy = GitProxy::new(&observer.get_path());
    let mut child = spawn_reading_with_path(&repo, "reindex", proxy.get_path());
    child.send_input(b"{}");
    // When
    let wrapper = WrapperConnection::accept(observer.get_listener());
    let lock = repo.get_root().join(".letopis/index.lock");
    assert_index_lock_held(&lock);
    child.send_sigint();
    let output = child.collect_output(WAIT);
    // Then: the abandoned Git child keeps neither the response pipe nor the index lock.
    assert_cancelled_output(&output, "git waiter", &["ls-files", CANCELLATION_CANARY]);
    wait_until_index_lock_free(&lock);
    wrapper.assert_alive();
    wrapper.release_and_wait();
}

#[test]
fn cancels_while_provider_request_is_pending() {
    // Given: a published node, a ready vector identity and a provider that holds the request.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", CANCELLATION_CANARY);
    let published = repo.invoke(&["reindex"], b"{}");
    assert_eq!(published.status.code(), Some(0), "{published:?}");
    let barrier = ResponseBarrier::default();
    let provider = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &provider.get_base_url(), "fixture");
    repo.seed_vectors(&config, &[("knowledge.md", &[1.0, 0.0])]);
    let mut child = spawn_reading(&repo, "search");
    let request = json!({"query": CANCELLATION_CANARY, "limit": 1}).to_string();
    child.send_input(request.as_bytes());
    // When: the fixture acknowledges the complete request before signalling.
    barrier.wait_until_entered();
    let captured = provider.take_requests();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0]["input"], json!([CANCELLATION_CANARY]));
    child.send_sigint();
    let output = child.collect_output(WAIT);
    // Then: no request text is echoed and no second provider request starts.
    assert_cancelled_output(
        &output,
        "provider wait",
        &[CANCELLATION_CANARY, "knowledge.md"],
    );
    barrier.release();
    assert!(provider.take_requests().is_empty());
}

fn spawn_reading(repo: &ReadingRepo, command: &str) -> cli::OwnedChild {
    let path = std::env::var_os("PATH").unwrap_or_default();
    spawn_reading_with_path(repo, command, &path)
}

fn spawn_reading_with_path(repo: &ReadingRepo, command: &str, path: &OsStr) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    process
        .current_dir(repo.get_root())
        .arg(command)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut process)
}
