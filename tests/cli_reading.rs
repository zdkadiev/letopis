use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead};
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
        EmbeddingExecutor, FileLocker, GitSnapshots, NodeMetadata, Refresher, VectorOutcome,
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
    // Given
    for filters in [
        json!({"needs_recheck":true}),
        json!({"needs_recheck":false}),
        json!({"sources":["src/x.rs"]}),
    ] {
        let repo = ReadingRepo::new();
        let body = json!({"query":"x","limit":1,"filters":filters}).to_string();

        // When
        let response = get_response(&repo.invoke(&["search"], body.as_bytes()), 0);

        // Then
        assert_eq!(response, json!({"ok":true,"result":[]}));
        assert!(repo.get_root().join(".letopis/cache.db").exists());
    }
    let repo = ReadingRepo::new();
    let tree = open_git_snapshots(repo.get_root())
        .unwrap()
        .tree_create(&[])
        .unwrap();
    let body =
        json!({"query":"x","limit":1,"filters":{"source_revisions":[tree.as_str()]}}).to_string();

    let response = get_response(&repo.invoke(&["search"], body.as_bytes()), 0);

    assert_eq!(response, json!({"ok":true,"result":[]}));
    assert!(repo.get_root().join(".letopis/cache.db").exists());
}

#[test]
fn malformed_filters_fail_before_repository_initialization() {
    // Given
    for filters in [
        json!({"sources":null}),
        json!({"needs_recheck":null}),
        json!({"source_revisions":["BAD"]}),
        json!({"unknown":1}),
    ] {
        let repo = ReadingRepo::new();
        let body = json!({"query":"x","limit":1,"filters":filters}).to_string();
        // When
        let response = get_response(&repo.invoke(&["search"], body.as_bytes()), 1);
        emit_reading("malformed_input", "search", 1, &response);
        // Then
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(!repo.get_root().join(".letopis").exists());
    }
}

#[test]
fn binary_search_refreshes_and_returns_links_with_empty_filter_equivalence() {
    // Given: published knowledge and equivalent empty filters.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "SQLite OR блокировки café 42");
    for suffix in [
        "",
        r#", "filters":{}"#,
        r#", "filters":{"sources":[],"source_revisions":[]}"#,
    ] {
        let body = format!(r#"{{"query":"SQLite OR блокировки café 42","limit":1{suffix}}}"#);
        // When: binary search refreshes the corpus.
        let response = get_response(&repo.invoke(&["search"], body.as_bytes()), 0);
        emit_reading("search_success", "search", 0, &response);
        // Then: links are returned with equivalent empty-filter behavior.
        assert_eq!(
            response,
            json!({"ok":true,"result":[{"element_id":"knowledge.md","path":"knowledge.md","lines":[1,2],"revision":1,"flags":[]}]})
        );
    }
    assert_eq!(
        {
            let scope_databases = repo.open_storage();
            scope_databases
                .cache
                .with_read_tx(|| Ok::<_, StorageError>(scope_databases.cache.project_get_by_id()))
                .unwrap()
        }
        .unwrap(),
        1
    );
}

#[test]
fn binary_search_refreshes_returns_links_and_triggers_existing_drain() {
    // Given: published knowledge and pending embedding work.
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
    // When: binary search refreshes and searches.
    let response = get_response(&child.collect_output(Duration::from_secs(10)), 0);
    // Then: links are returned and the existing drain starts.
    assert_eq!(response["result"][0]["element_id"], "knowledge.md");
    assert!(response.get("warnings").is_none());
    barrier.wait_until_entered();
    assert!(owner.get_pid() > 0);
    barrier.release();
    owner.wait_for_exit();
    assert_eq!(
        server.take_requests(),
        vec![json!({"model":"fixture","input":["sqlite"],"truncate":false})]
    );
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            let nodes = read.nodes_get().unwrap();
            let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
                panic!("knowledge")
            };
            assert_eq!(
                read.vector_keys_get_vector_by_search_hash(
                    &config.get_identity(),
                    &node.search_hash
                )
                .unwrap(),
                VectorOutcome::Current(Some(vec![1.0, 0.0]))
            );

            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn binary_inspect_returns_current_retained_tree_metadata() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("knowledge.md", include_bytes!("fixtures/markdown/crlf.md"));
    // When
    let response = get_response(
        &repo.invoke(
            &["inspect"],
            br#"{"element_ids":["knowledge.md#a","./knowledge.md#a"]}"#,
        ),
        0,
    );
    emit_reading("inspect_success", "inspect", 0, &response);
    // Then
    assert_eq!(
        response["result"]["elements"],
        json!({"knowledge.md#a":{"path":"knowledge.md","lines":[1,3],"hash":"eacc729e917594b0955d7ccaae6b41106a58c53cc7b9d58d7d186f957ad72864"}})
    );
    let tree = TreeOid::parse(response["result"]["snapshot"].as_str().unwrap()).unwrap();
    repo.write("knowledge.md", b"later live edit");
    let git = open_git_snapshots(repo.get_root()).unwrap();
    assert_eq!(
        git.blob_get_by_tree_oid_and_path(&tree, &RepoPath::parse("knowledge.md").unwrap())
            .unwrap(),
        include_bytes!("fixtures/markdown/crlf.md")
    );
}

#[test]
fn binary_inspect_returns_mixed_metadata_without_text_or_flags() {
    // Given
    let repo = ReadingRepo::new();
    let knowledge = b"<!-- letopis: {\"created_at\":1792800000,\"source_snapshot\":\"0000000000000000000000000000000000000000\"} -->\nCache decision.\n[range](src/cache.rs#L2-L2 \"letopis:type=external;path=src/cache.rs#L2-L2\")\n[missing](src/missing.rs \"letopis:type=external;path=src/missing.rs\")\n[remote](https://example.invalid/a?q=1#fragment \"letopis:type=external;path=https://example.invalid/a?q=1#fragment\")\n";
    repo.write("knowledge.md", knowledge);
    repo.write("src/cache.rs", b"alpha\r\nbeta\n");
    let body = br#"{"element_ids":["knowledge.md","src/cache.rs#L2-L2","src/missing.rs","https://example.invalid/a?q=1#fragment","./knowledge.md"]}"#;

    // When
    let response = get_response(&repo.invoke(&["inspect"], body), 0);

    emit_reading("inspect_mixed", "inspect", 0, &response);
    // Then
    assert_eq!(response["ok"], true);
    assert_eq!(
        response["result"]["elements"],
        json!({
            "knowledge.md":{"path":"knowledge.md","lines":[1,5],"hash":"5e66872103f70b0974a295a78645f384c6f49c716cfbaa2131f451c617a947bb"},
            "src/cache.rs#L2-L2":{"path":"src/cache.rs","lines":[2,2],"hash":"f2c82decdd7181cf98945929a62598db7e6b477e11f6e0eb0ae97020eff151ad"},
            "src/missing.rs":{"path":"src/missing.rs","hash":null},
            "https://example.invalid/a?q=1#fragment":{"path":"https://example.invalid/a?q=1#fragment","hash":null}
        })
    );
    let tree = TreeOid::parse(response["result"]["snapshot"].as_str().unwrap()).unwrap();
    let git = open_git_snapshots(repo.get_root()).unwrap();
    assert_eq!(
        git.blob_get_by_tree_oid_and_path(&tree, &RepoPath::parse("knowledge.md").unwrap())
            .unwrap(),
        knowledge
    );
    assert_eq!(
        git.blob_get_by_tree_oid_and_path(&tree, &RepoPath::parse("src/cache.rs").unwrap())
            .unwrap(),
        b"alpha\r\nbeta\n"
    );
    assert!(
        git.blob_get_by_tree_oid_and_path(&tree, &RepoPath::parse("src/missing.rs").unwrap())
            .is_err()
    );
}

#[test]
fn binary_inspect_rejects_unknown_member_without_partial_result() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "knowledge.md",
        "Private fixture secret.\n[source](src/cache.rs \"letopis:type=external;path=src/cache.rs\")",
    );
    repo.write("src/cache.rs", b"private source secret\n");
    let initial = br#"{"element_ids":["knowledge.md","src/cache.rs"]}"#;
    // When / Then
    assert_eq!(
        get_response(&repo.invoke(&["inspect"], initial), 0)["ok"],
        true
    );
    repo.write_knowledge("knowledge.md", "Private fixture secret after deletion.");

    for unknown in ["src/cache.rs", "knowledge.md#removed"] {
        let body = json!({"element_ids":["knowledge.md",unknown]}).to_string();
        let output = repo.invoke(&["inspect"], body.as_bytes());
        let response = get_response(&output, 1);
        emit_reading("inspect_removed", "inspect", 1, &response);
        assert_eq!(response["error"]["code"], "not_found");
        assert!(response.get("result").is_none());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Private fixture secret"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private source secret"));
    }
    fs::remove_file(repo.get_root().join("knowledge.md")).unwrap();
    let response = get_response(&repo.invoke(&["inspect"], initial), 1);
    assert_eq!(response["error"]["code"], "not_found");
    assert!(response.get("result").is_none());
}

#[test]
fn binary_query_failure_is_plain_fts_success_with_safe_persisted_error() {
    // Given: a query provider that fails.
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
        // When: binary search uses lexical fallback.
        let response = get_response(
            &repo.invoke(&["search"], br#"{"query":"sqlite","limit":1}"#),
            0,
        );
        // Then: search succeeds and persists a safe query error.
        assert_eq!(
            response,
            json!({"ok":true,"result":[{"element_id":"knowledge.md","path":"knowledge.md","lines":[1,2],"revision":1,"flags":[]}]})
        );
        emit_reading("search_fallback", "search", 0, &response);
        let requests = server.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["input"], json!(["sqlite"]));
        let storage = repo.open_storage();
        storage
            .cache
            .with_read_tx(|| {
                let read = &storage.cache;
                let error = read.query_error_get_by_id().unwrap().unwrap();
                assert!(!error.error.contains("PRIVATE_PROVIDER_BODY"));
                assert!(!error.error.contains("fixture-secret"));
                assert_eq!(error.error, "embedding provider rejected request");
                let nodes = read.nodes_get().unwrap();
                let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
                    panic!("knowledge")
                };
                assert_eq!(
                    read.embed_failures_get_by_search_hash(
                        &config.get_identity(),
                        &node.search_hash
                    )
                    .unwrap(),
                    VectorOutcome::Current(None)
                );

                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}

#[test]
fn binary_inspect_unknown_is_all_or_nothing_and_missing_source_has_null_hash() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "knowledge.md",
        "sqlite\n[source](source.rs \"letopis:type=external;path=source.rs\")",
    );
    for ids in [vec!["knowledge.md", "missing.md"], vec!["unknown.rs"]] {
        let body = json!({"element_ids":ids}).to_string();
        // When
        let response = get_response(&repo.invoke(&["inspect"], body.as_bytes()), 1);
        emit_reading("not_found", "inspect", 1, &response);
        // Then
        assert_eq!(response["error"]["code"], "not_found");
        assert!(response.get("result").is_none());
    }
    let body = json!({"element_ids":["knowledge.md", "source.rs"]}).to_string();
    let response = get_response(&repo.invoke(&["inspect"], body.as_bytes()), 0);
    assert_eq!(
        response["result"]["elements"]["source.rs"]["path"],
        "source.rs"
    );
    assert!(response["result"]["elements"]["source.rs"]["hash"].is_null());
    repo.write("knowledge.md", b"<!-- letopis: {broken} -->\ninvalid");
    assert_eq!(
        get_response(
            &repo.invoke(&["inspect"], br#"{"element_ids":["knowledge.md"]}"#),
            1
        )["error"]["code"],
        "validation_failed"
    );
}

#[test]
fn lost_output_then_retry_preserves_corpus_user_state_and_revision() {
    // Given: a command whose first output is lost.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "sqlite");
    run_git(&repo, &["update-ref", "refs/user/keep", "HEAD"]);
    let before = capture_user_state(&repo);
    // When: the client retries the command.
    for (args, body) in [
        ("search", br#"{"query":"sqlite","limit":1}"#.as_slice()),
        ("inspect", br#"{"element_ids":["knowledge.md"]}"#.as_slice()),
    ] {
        let lost = repo.invoke_lost(&[args], body);
        // Then: corpus, user state and revision remain unchanged.
        assert_eq!(lost.status.code(), Some(0));
        assert!(lost.stderr.is_empty());
        for _ in 0..2 {
            assert_eq!(get_response(&repo.invoke(&[args], body), 0)["ok"], true);
        }
        assert_eq!(
            {
                let scope_databases = repo.open_storage();
                scope_databases
                    .cache
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(scope_databases.cache.project_get_by_id())
                    })
                    .unwrap()
            }
            .unwrap(),
            1
        );
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
        // Given: actual eligible work and an existing root drain lease.
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
        let _lease = files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .unwrap();
        let child_service = repo.get_root().join("subdir/.letopis");
        fs::write(&child_service, b"keep unsafe child service path").unwrap();
        // When: the nested cwd must not change the lock or service directory owner.
        let mut child = spawn_nested_reading(&repo, command);
        child.send_input(input);
        let response = get_response(&child.collect_output(Duration::from_secs(10)), 0);
        // Then: root exclusion avoids both provider calls and launch warnings.
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
    // Given
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
        // When
        let response = get_response(&child.collect_output(Duration::from_secs(10)), 0);
        // HTTP arrival establishes ownership before inspecting the root PID receipt.
        barrier.wait_until_entered();
        // Then
        assert!(owner.get_pid() > 0);
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

fn get_response(output: &Output, exit: i32) -> Value {
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
