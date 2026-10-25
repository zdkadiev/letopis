#[expect(
    dead_code,
    reason = "Shared process fixture also exposes unrelated runtime helpers"
)]
#[path = "support/cli.rs"]
mod cli;
#[expect(
    dead_code,
    reason = "Only detached ownership is needed from shared drain fixtures"
)]
#[path = "support/drain.rs"]
mod drain;
mod support;
use letopis::{
    domain::{RepoPath, TreeOid},
    open_filesystem, open_git_snapshots,
    service::{
        EmbedLocker, EmbeddingExecutor, GitSnapshots, NodeMetadata, Refresher, Storage,
        StorageRead, VectorOutcome,
    },
    wire_document_refresher, wire_embedding_executor,
};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output, Stdio},
    time::Duration,
};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    git::isolate_command,
    reading::ReadingRepo,
};

#[test]
fn effective_filters_initialize_repository_for_search() {
    for filters in [
        json!({"needs_recheck":true}),
        json!({"needs_recheck":false}),
        json!({"sources":["src/x.rs"]}),
    ] {
        let repo = ReadingRepo::new();
        let body = json!({"query":"x","limit":1,"filters":filters}).to_string();

        let response = read_response(&repo.invoke(&["search"], body.as_bytes()), 0);

        assert_eq!(response, json!({"ok":true,"result":[]}));
        assert!(repo.get_root().join(".letopis/cache.db").exists());
    }
    let repo = ReadingRepo::new();
    let tree = open_git_snapshots(repo.get_root())
        .unwrap()
        .create_tree(&[])
        .unwrap();
    let body =
        json!({"query":"x","limit":1,"filters":{"source_revisions":[tree.as_str()]}}).to_string();

    let response = read_response(&repo.invoke(&["search"], body.as_bytes()), 0);

    assert_eq!(response, json!({"ok":true,"result":[]}));
    assert!(repo.get_root().join(".letopis/cache.db").exists());
}

#[test]
fn malformed_filters_fail_before_repository_initialization() {
    for filters in [
        json!({"sources":null}),
        json!({"needs_recheck":null}),
        json!({"source_revisions":["BAD"]}),
        json!({"unknown":1}),
    ] {
        let repo = ReadingRepo::new();
        let body = json!({"query":"x","limit":1,"filters":filters}).to_string();
        let response = read_response(&repo.invoke(&["search"], body.as_bytes()), 1);
        emit_reading("malformed_input", "search", 1, &response);
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(!repo.get_root().join(".letopis").exists());
    }
}

#[test]
fn binary_search_refreshes_and_returns_links_with_empty_filter_equivalence() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "SQLite OR блокировки café 42");
    for suffix in [
        "",
        r#", "filters":{}"#,
        r#", "filters":{"sources":[],"source_revisions":[]}"#,
    ] {
        let body = format!(r#"{{"query":"SQLite OR блокировки café 42","limit":1{suffix}}}"#);
        let response = read_response(&repo.invoke(&["search"], body.as_bytes()), 0);
        emit_reading("search_success", "search", 0, &response);
        assert_eq!(
            response,
            json!({"ok":true,"result":[{"element_id":"knowledge.md","path":"knowledge.md","lines":[1,2],"revision":1,"flags":[]}]})
        );
    }
    assert_eq!(repo.open_storage().begin_read().unwrap().get_revision(), 1);
}

#[test]
fn binary_search_refreshes_returns_links_and_triggers_existing_drain() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "sqlite");
    let barrier = ResponseBarrier::default();
    let server = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    let mut child = cli::OwnedChild::spawn(
        Command::new(env!("CARGO_BIN_EXE_letopis"))
            .current_dir(repo.get_root())
            .arg("search")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    let mut owner = drain::DetachedOwner::new(repo.get_root(), child.get_id());
    child.send_input(br#"{"query":"sqlite","limit":1}"#);
    let response = read_response(&child.collect_output(Duration::from_secs(10)), 0);
    assert_eq!(response["result"][0]["element_id"], "knowledge.md");
    assert!(response.get("warnings").is_none());
    barrier.wait_until_entered();
    assert!(owner.read_pid() > 0);
    barrier.release();
    owner.wait_for_exit();
    assert_eq!(
        server.take_requests(),
        vec![json!({"model":"fixture","input":["sqlite"],"truncate":false})]
    );
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    let nodes = read.list_nodes().unwrap();
    let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
        panic!("knowledge")
    };
    assert_eq!(
        read.read_vector(&config.get_identity(), &node.search_hash)
            .unwrap(),
        VectorOutcome::Current(Some(vec![1.0, 0.0]))
    );
}

#[test]
fn binary_inspect_returns_current_retained_tree_metadata() {
    let repo = ReadingRepo::new();
    repo.write("knowledge.md", include_bytes!("fixtures/markdown/crlf.md"));
    let response = read_response(
        &repo.invoke(
            &["inspect"],
            br#"{"element_ids":["knowledge.md#a","./knowledge.md#a"]}"#,
        ),
        0,
    );
    emit_reading("inspect_success", "inspect", 0, &response);
    assert_eq!(
        response["result"]["elements"],
        json!({"knowledge.md#a":{"path":"knowledge.md","lines":[1,3],"hash":"eacc729e917594b0955d7ccaae6b41106a58c53cc7b9d58d7d186f957ad72864"}})
    );
    let tree = TreeOid::parse(response["result"]["snapshot"].as_str().unwrap()).unwrap();
    repo.write("knowledge.md", b"later live edit");
    let git = open_git_snapshots(repo.get_root()).unwrap();
    assert_eq!(
        git.read_blob(&tree, &RepoPath::parse("knowledge.md").unwrap())
            .unwrap(),
        include_bytes!("fixtures/markdown/crlf.md")
    );
}

#[test]
fn binary_query_failure_is_plain_fts_success_with_safe_persisted_error() {
    for provider in ["ollama", "openai"] {
        let repo = ReadingRepo::new();
        repo.write_knowledge("knowledge.md", "sqlite");
        let server = HttpFixture::start(FixtureReply::Status {
            status: 500,
            body: "PRIVATE_PROVIDER_BODY".into(),
        });
        let config = repo.write_config(provider, &server.get_base_url(), "fixture");
        wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap();
        repo.seed_vectors(&config, &[("knowledge.md", &[1.0, 0.0])]);
        let response = read_response(
            &repo.invoke(&["search"], br#"{"query":"sqlite","limit":1}"#),
            0,
        );
        assert_eq!(
            response,
            json!({"ok":true,"result":[{"element_id":"knowledge.md","path":"knowledge.md","lines":[1,2],"revision":1,"flags":[]}]})
        );
        emit_reading("search_fallback", "search", 0, &response);
        let requests = server.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["input"], json!(["sqlite"]));
        let mut storage = repo.open_storage();
        let mut read = storage.begin_read().unwrap();
        let error = read.read_query_error().unwrap().unwrap();
        assert!(!error.error.contains("PRIVATE_PROVIDER_BODY"));
        assert!(!error.error.contains("fixture-secret"));
        assert_eq!(error.error, "embedding provider rejected request");
        let nodes = read.list_nodes().unwrap();
        let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
            panic!("knowledge")
        };
        assert_eq!(
            read.read_failure(&config.get_identity(), &node.search_hash)
                .unwrap(),
            VectorOutcome::Current(None)
        );
    }
}

#[test]
fn binary_inspect_unknown_and_source_errors_are_all_or_nothing() {
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "knowledge.md",
        "sqlite\n[source](source.rs \"letopis:type=external\")",
    );
    for (ids, code) in [
        (vec!["knowledge.md", "missing.md"], "not_found"),
        (vec!["knowledge.md", "source.rs"], "internal_error"),
        (vec!["unknown.rs"], "not_found"),
    ] {
        let body = json!({"element_ids":ids}).to_string();
        let response = read_response(&repo.invoke(&["inspect"], body.as_bytes()), 1);
        if code == "not_found" {
            emit_reading("not_found", "inspect", 1, &response);
        }
        assert_eq!(response["error"]["code"], code);
        assert!(response.get("result").is_none());
    }
    repo.write("knowledge.md", b"<!-- letopis: {broken} -->\ninvalid");
    assert_eq!(
        read_response(
            &repo.invoke(&["inspect"], br#"{"element_ids":["knowledge.md"]}"#),
            1
        )["error"]["code"],
        "validation_failed"
    );
}

#[test]
fn lost_output_then_retry_preserves_corpus_user_state_and_revision() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "sqlite");
    run_git(&repo, &["update-ref", "refs/user/keep", "HEAD"]);
    let before = capture_user_state(&repo);
    for (args, body) in [
        ("search", br#"{"query":"sqlite","limit":1}"#.as_slice()),
        ("inspect", br#"{"element_ids":["knowledge.md"]}"#.as_slice()),
    ] {
        let lost = repo.invoke_lost(&[args], body);
        assert_eq!(lost.status.code(), Some(0));
        assert!(lost.stderr.is_empty());
        for _ in 0..2 {
            assert_eq!(read_response(&repo.invoke(&[args], body), 0)["ok"], true);
        }
        assert_eq!(repo.open_storage().begin_read().unwrap().get_revision(), 1);
        assert_eq!(capture_user_state(&repo), before);
    }
}

#[test]
fn nested_read_commands_honor_root_lock_and_ignore_poisoned_child_service_path() {
    let mut warnings = Vec::new();
    for (command, input) in [
        ("search", br#"{"query":"sqlite","limit":1}"#.as_slice()),
        ("inspect", br#"{"element_ids":["knowledge.md"]}"#.as_slice()),
    ] {
        // Arrange: actual eligible work and an existing root drain lease.
        let repo = ReadingRepo::new();
        repo.write_knowledge("knowledge.md", "sqlite");
        let http = HttpFixture::start(FixtureReply::Status {
            status: 200,
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        });
        repo.write_config("ollama", &http.get_base_url(), "fixture");
        wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap();
        assert!(
            wire_embedding_executor(repo.get_root())
                .unwrap()
                .has_work()
                .unwrap()
        );
        let files = open_filesystem(repo.get_root()).unwrap();
        let _lease = files.try_lock_embed().unwrap().unwrap();
        let child_service = repo.get_root().join("subdir/.letopis");
        fs::write(&child_service, b"keep unsafe child service path").unwrap();
        // Act: the nested cwd must not change the lock or service directory owner.
        let mut child = spawn_nested_reading(&repo, command);
        child.send_input(input);
        let response = read_response(&child.collect_output(Duration::from_secs(10)), 0);
        // Assert: root exclusion avoids both provider calls and launch warnings.
        assert_eq!(response["ok"], true);
        warnings.push((command, response.get("warnings").cloned()));
        assert_eq!(
            fs::read(&child_service).unwrap(),
            b"keep unsafe child service path"
        );
        assert!(http.take_requests().is_empty());
        assert!(
            wire_embedding_executor(repo.get_root())
                .unwrap()
                .has_work()
                .unwrap()
        );
    }
    assert_eq!(warnings, [("search", None), ("inspect", None)]);
}

#[test]
fn nested_search_and_inspect_launch_the_existing_drain_only_at_repository_root() {
    let mut child_service_paths = Vec::new();
    for (command, input) in [
        ("search", br#"{"query":"sqlite","limit":1}"#.as_slice()),
        ("inspect", br#"{"element_ids":["knowledge.md"]}"#.as_slice()),
    ] {
        let repo = ReadingRepo::new();
        repo.write_knowledge("knowledge.md", "sqlite");
        let barrier = ResponseBarrier::default();
        let http = HttpFixture::start(FixtureReply::Barrier {
            barrier: barrier.clone(),
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        });
        repo.write_config("ollama", &http.get_base_url(), "fixture");
        let mut child = spawn_nested_reading(&repo, command);
        let mut owner = drain::DetachedOwner::new(repo.get_root(), child.get_id());
        child.send_input(input);
        let response = read_response(&child.collect_output(Duration::from_secs(10)), 0);
        // HTTP arrival establishes ownership before inspecting the root PID receipt.
        barrier.wait_until_entered();
        assert!(owner.read_pid() > 0);
        barrier.release();
        owner.wait_for_exit();
        assert_eq!(response["ok"], true);
        assert!(response.get("warnings").is_none(), "{response}");
        child_service_paths.push((command, repo.get_root().join("subdir/.letopis").exists()));
        assert_eq!(
            http.take_requests(),
            vec![json!({"model":"fixture","input":["sqlite"],"truncate":false})]
        );
        assert!(
            !wire_embedding_executor(repo.get_root())
                .unwrap()
                .has_work()
                .unwrap()
        );
    }
    assert_eq!(child_service_paths, [("search", false), ("inspect", false)]);
}

fn spawn_nested_reading(repo: &ReadingRepo, command: &str) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    process
        .current_dir(repo.get_root().join("subdir"))
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut process)
}

fn read_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn emit_reading(case: &str, command: &str, exit: i32, response: &Value) {
    println!(
        "PRODUCT_READING {}",
        json!({"case":case,"command":command,"exit":exit,"response":response})
    );
}
fn capture_user_state(repo: &ReadingRepo) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let refs = run_git(repo, &["for-each-ref"]);
    let user_ref_lines = refs.split_inclusive(|byte| *byte == b'\n').filter(|line| {
        !line
            .windows(b"refs/letopis/".len())
            .any(|part| part == b"refs/letopis/")
    });
    let user_refs = user_ref_lines.flatten().copied().collect();
    (
        fs::read(repo.get_root().join("knowledge.md")).unwrap(),
        fs::read(repo.get_root().join(".git/index")).unwrap(),
        fs::read(repo.get_root().join(".git/HEAD")).unwrap(),
        user_refs,
    )
}
fn run_git(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}
