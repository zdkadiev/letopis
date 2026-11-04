#[expect(
    dead_code,
    reason = "Shared cancellation fixture exposes helpers used by other CLI tests"
)]
#[path = "support/cancellation.rs"]
mod cancellation;
#[expect(
    dead_code,
    reason = "Shared owned-process fixture exposes unrelated CLI runtime helpers"
)]
#[path = "support/cli.rs"]
mod cli;
mod support;

use cancellation::{GitReadBarrier, WAIT, assert_cancelled_output, wait_until_index_lock_free};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};
use support::{
    embedding::{FixtureReply, HttpFixture},
    git::isolate_command,
    reading::ReadingRepo,
};

const DECISION_PATH: &str = "knowledge/decisions.md";
const DECISION_ID: &str = "knowledge/decisions.md#cache";
const RELATED_PATH: &str = "knowledge/other.md";
const RELATED_ID: &str = "knowledge/other.md#decision";
const SOURCE_PATH: &str = "src/cache.rs";
const SOURCE_ID: &str = "src/cache.rs#L2-L4";
const SOURCE: &[u8] = b"fn cache() {\n    println!(\"checked\");\n    println!(\"stable\");\n}\n";
const CACHE_TOKEN: &str = "zqxdecisioncanary";
const RELATED_TOKEN: &str = "vbnrelatedcanary";
const CONFIG_CANARY: &str = "TK29_CONFIG_CANARY";

#[test]
fn maintenance_roundtrip_preserves_knowledge_and_source_basis() {
    // Given: a disposable repository with verified source bytes and marked knowledge.
    let workflow = Workflow::create();
    let before = workflow.capture_authored_state();

    // When / Then: observation before setup leaves no service state behind.
    assert_config_absent(&workflow);

    // When: the agent writes a valid local configuration, then reads, observes and reindexes.
    let provider = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1.0,0.0]]}"#.into(),
    });
    configure_and_publish(&workflow, &provider.get_base_url());

    // When / Then: normal refresh, search, traversal and mixed inspection share one snapshot.
    assert_reading_workflow(&workflow);

    // When / Then: invalid configuration fails safely while FTS stays available.
    assert_invalid_configuration(&workflow);

    // When / Then: restoring the file fixes observation and the next refresh succeeds.
    assert_repaired_configuration(&workflow, &provider.get_base_url());

    // When / Then: a cancelled refresh is one envelope, exit 1 and retryable.
    assert_cancelled_refresh_is_retryable(&workflow);

    // Then: no CLI invocation edited authored Markdown, the Git index or HEAD.
    assert_eq!(
        workflow.capture_authored_state(),
        before,
        "CLI must not modify authored files or Git state"
    );
}

struct Workflow {
    repo: ReadingRepo,
    source_snapshot: String,
}

impl Workflow {
    /// Create a Git repository whose committed source tree is already verified.
    fn create() -> Self {
        let repo = ReadingRepo::new();
        repo.write(SOURCE_PATH, SOURCE);
        run_git(&repo, &["add", SOURCE_PATH]);
        run_git(&repo, &["commit", "-qm", "checked source"]);
        let source_snapshot = get_git_text(&repo, &["rev-parse", "HEAD^{tree}"]);
        assert_eq!(
            get_git_bytes(
                &repo,
                &["show", &format!("{source_snapshot}:{SOURCE_PATH}")]
            ),
            SOURCE
        );
        let decision = format!(
            "## Cache choice {{#cache}}\n\
             <!-- letopis: {{\"created_at\":1792800000,\"source_snapshot\":\"{source_snapshot}\"}} -->\n\
             {CACHE_TOKEN} follows the checked code.\n\
             [code](../src/cache.rs#L2-L4 \"letopis:type=external;path={SOURCE_ID}\")\n\
             [related](other.md#decision \"letopis:type=depends_on;path={RELATED_ID}\")\n"
        );
        let related = format!(
            "## Decision {{#decision}}\n\
             <!-- letopis: {{\"created_at\":1792800001}} -->\n\
             {RELATED_TOKEN} supports the cache choice.\n"
        );
        repo.write(DECISION_PATH, decision.as_bytes());
        repo.write(RELATED_PATH, related.as_bytes());
        Self {
            repo,
            source_snapshot,
        }
    }

    /// Invoke the shipped executable and emit the raw envelope for host schema validation.
    fn invoke_command(&self, command: &str, input: &[u8], expected: i32) -> Value {
        let output = self.repo.invoke(&[command], input);
        get_response(command, &output, expected)
    }

    /// Read one captured tree path through Git without consulting live files.
    fn get_snapshot(&self, snapshot: &str, path: &str) -> Vec<u8> {
        get_git_bytes(&self.repo, &["show", &format!("{snapshot}:{path}")])
    }

    /// Capture bytes the CLI must never edit: authored source, Markdown, index and HEAD.
    fn capture_authored_state(&self) -> AuthoredState {
        let root = self.repo.get_root();
        let files = [SOURCE_PATH, DECISION_PATH, RELATED_PATH]
            .into_iter()
            .map(|path| (path.to_owned(), fs::read(root.join(path)).unwrap()))
            .collect();
        AuthoredState {
            files,
            index: fs::read(root.join(".git/index")).unwrap(),
            head: fs::read(root.join(".git/HEAD")).unwrap(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AuthoredState {
    files: BTreeMap<String, Vec<u8>>,
    index: Vec<u8>,
    head: Vec<u8>,
}

fn assert_config_absent(workflow: &Workflow) {
    let observed = workflow.invoke_command("config", b"{}", 0);
    assert_eq!(observed, json!({"ok":true,"result":null}));
    assert!(!workflow.repo.get_root().join(".letopis").exists());
}

fn configure_and_publish(workflow: &Workflow, endpoint: &str) {
    let config = workflow.repo.write_config("ollama", endpoint, "fixture");
    let observed = workflow.invoke_command("config", b"{}", 0);
    assert_eq!(
        observed["result"],
        json!({"embedding_model":{
            "enabled":true,
            "provider":"ollama",
            "model":"fixture",
            "parameters":{"base_url":endpoint},
            "max_retries":0
        }})
    );
    let status = workflow.invoke_command("status", b"{}", 0);
    assert_eq!(status["result"]["revision"], Value::Null);
    assert_eq!(status["result"]["problems"], json!([]));
    // The fixture seeds vectors itself; exclude the detached drain while owning that setup.

    use letopis::service::FileLocker;
    let files = letopis::open_filesystem(workflow.repo.get_root()).unwrap();
    let _embedding_setup = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    assert_eq!(
        workflow.invoke_command("reindex", b"{}", 0),
        json!({"ok":true,"result":{"revision":1}})
    );
    workflow.repo.seed_vectors(
        &config,
        &[(DECISION_ID, &[1.0, 0.0]), (RELATED_ID, &[0.0, 1.0])],
    );
    let status = workflow.invoke_command("status", b"{}", 0);
    assert_eq!(status["result"]["revision"], 1);
    assert!(status["result"]["embedding_backlog"].is_u64());
    assert!(status["result"]["locks"].is_array());
    assert_eq!(status["result"]["problems"], json!([]));
}

fn assert_reading_workflow(workflow: &Workflow) {
    let checked = workflow.invoke_command("check", b"{}", 0);
    assert_eq!(checked, json!({"ok":true,"result":{"errors":[]}}));
    let searched = workflow.invoke_command(
        "search",
        format!(r#"{{"query":"{CACHE_TOKEN}","limit":5}}"#).as_bytes(),
        0,
    );
    let results = searched["result"].as_array().unwrap();
    let decision = results
        .iter()
        .find(|link| link["element_id"] == DECISION_ID)
        .unwrap_or_else(|| panic!("decision missing from {searched}"));
    assert_eq!(decision["revision"], 1);
    assert!(!searched.to_string().contains(CACHE_TOKEN));
    assert_traversal(workflow);
    assert_mixed_inspection(workflow);
}

fn assert_traversal(workflow: &Workflow) {
    let body = json!({
        "element_ids":[DECISION_ID],
        "mode":"both",
        "direction":"outgoing",
        "depth":1,
        "limit":10,
        "edge_limit":10
    });
    let traversed = workflow.invoke_command("traverse", body.to_string().as_bytes(), 0);
    let elements = traversed["result"]["elements"].as_array().unwrap();
    let ids = elements
        .iter()
        .map(|element| element["element_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, [RELATED_ID, SOURCE_ID]);
    let source = elements
        .iter()
        .find(|element| element["element_id"] == SOURCE_ID)
        .unwrap();
    assert_eq!(source["path"], SOURCE_PATH);
    assert_eq!(source["lines"], json!([2, 4]));
    assert_eq!(source["flags"], json!([]));
    let edges = traversed["result"]["edges"].as_array().unwrap();
    for (target, edge_type) in [(RELATED_ID, "depends_on"), (SOURCE_ID, "external")] {
        let expected = json!({"source":DECISION_ID,"target":target,"type":edge_type});
        assert!(edges.contains(&expected), "missing {expected}: {traversed}");
    }
}

fn assert_mixed_inspection(workflow: &Workflow) {
    let body = json!({"element_ids":[DECISION_ID, SOURCE_ID]});
    let inspected = workflow.invoke_command("inspect", body.to_string().as_bytes(), 0);
    let snapshot = inspected["result"]["snapshot"].as_str().unwrap();
    let knowledge = workflow.get_snapshot(snapshot, DECISION_PATH);
    let source = workflow.get_snapshot(snapshot, SOURCE_PATH);
    assert_eq!(
        knowledge,
        fs::read(workflow.repo.get_root().join(DECISION_PATH)).unwrap()
    );
    assert_eq!(source, SOURCE);
    assert_eq!(
        workflow.get_snapshot(&workflow.source_snapshot, SOURCE_PATH),
        SOURCE
    );
    let elements = &inspected["result"]["elements"];
    assert_eq!(elements[DECISION_ID]["hash"], hash_bytes(&knowledge));
    let addressed = select_lines(&source, 2, 4);
    assert_eq!(elements[SOURCE_ID]["hash"], hash_bytes(&addressed));
    assert_eq!(elements[SOURCE_ID]["path"], SOURCE_PATH);
    assert_eq!(elements[SOURCE_ID]["lines"], json!([2, 4]));
}

fn assert_invalid_configuration(workflow: &Workflow) {
    workflow.repo.write(
        ".letopis/config.json",
        format!("{CONFIG_CANARY} not json").as_bytes(),
    );
    let error = workflow.invoke_command("config", b"{}", 1);
    assert_eq!(
        error,
        json!({"ok":false,"error":{"code":"embedding_failed",
            "message":"invalid embedding configuration","details":{}}})
    );
    assert!(!error.to_string().contains(CONFIG_CANARY));
    let status = workflow.invoke_command("status", b"{}", 0);
    assert_eq!(status["result"]["revision"], 1);
    assert_problem_reason(&status, "invalid_embedding_config", true);
    let searched = workflow.invoke_command(
        "search",
        format!(r#"{{"query":"{CACHE_TOKEN}","limit":5}}"#).as_bytes(),
        0,
    );
    assert!(contains_link(&searched, DECISION_ID), "{searched}");
    assert!(!searched.to_string().contains(CONFIG_CANARY));
}

fn assert_repaired_configuration(workflow: &Workflow, endpoint: &str) {
    workflow.repo.write_config("ollama", endpoint, "fixture");
    let observed = workflow.invoke_command("config", b"{}", 0);
    assert_eq!(
        observed["result"]["embedding_model"]["parameters"]["base_url"],
        endpoint
    );
    let checked = workflow.invoke_command("check", b"{}", 0);
    assert_eq!(checked["result"]["errors"], json!([]));
    let status = workflow.invoke_command("status", b"{}", 0);
    assert_problem_reason(&status, "invalid_embedding_config", false);
    let searched = workflow.invoke_command(
        "search",
        format!(r#"{{"query":"{CACHE_TOKEN}","limit":5}}"#).as_bytes(),
        0,
    );
    assert!(contains_link(&searched, DECISION_ID), "{searched}");
}

fn assert_cancelled_refresh_is_retryable(workflow: &Workflow) {
    let barrier = GitReadBarrier::new(workflow.repo.get_root());
    let mut child = spawn_check_with_path(&workflow.repo, OsStr::new("/nonexistent-letopis-git"));
    child.send_input(b"{}");
    barrier.observe(child.get_id());
    child.send_sigint();
    let output = child.collect_output(WAIT);
    assert_cancelled_output(
        &output,
        "maintenance check",
        &[CACHE_TOKEN, RELATED_TOKEN, CONFIG_CANARY],
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    emit_envelope("check", 1, &response);
    let lock = workflow.repo.get_root().join(".letopis/index.lock");
    wait_until_index_lock_free(&lock);
    barrier.assert_owned();
    barrier.release();
    let retried = workflow.invoke_command("check", b"{}", 0);
    assert_eq!(retried, json!({"ok":true,"result":{"errors":[]}}));
}

fn spawn_check_with_path(repo: &ReadingRepo, path: &OsStr) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    process
        .current_dir(repo.get_root())
        .arg("check")
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut process)
}

fn get_response(command: &str, output: &Output, expected: i32) -> Value {
    assert_eq!(output.status.code(), Some(expected), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "{output:?}"
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    emit_envelope(command, expected, &response);
    response
}

fn emit_envelope(command: &str, exit: i32, response: &Value) {
    println!(
        "WORKFLOW_ENVELOPE {}",
        json!({"command":command,"exit":exit,"response":response})
    );
}

fn contains_link(response: &Value, id: &str) -> bool {
    response["result"]
        .as_array()
        .is_some_and(|links| links.iter().any(|link| link["element_id"] == id))
}

fn assert_problem_reason(status: &Value, reason: &str, expected: bool) {
    let found = status["result"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .any(|problem| problem["details"]["reason"] == reason);
    assert_eq!(found, expected, "{status}");
}

fn select_lines(bytes: &[u8], start: u64, end: u64) -> Vec<u8> {
    let mut selected = Vec::new();
    for (line, number) in bytes.split_inclusive(|byte| *byte == b'\n').zip(1_u64..) {
        if (start..=end).contains(&number) {
            selected.extend_from_slice(line);
        }
    }
    selected
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn run_git(repo: &ReadingRepo, args: &[&str]) {
    let output = build_git_command(repo.get_root(), args).output().unwrap();
    assert!(
        output.status.success(),
        "fixture Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn get_git_text(repo: &ReadingRepo, args: &[&str]) -> String {
    String::from_utf8(get_git_bytes(repo, args))
        .unwrap()
        .trim()
        .to_owned()
}

fn get_git_bytes(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let output = build_git_command(repo.get_root(), args).output().unwrap();
    assert!(
        output.status.success(),
        "fixture Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn build_git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    command.current_dir(root).args(args);
    command
}
