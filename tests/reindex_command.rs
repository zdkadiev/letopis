mod support;
use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    open_filesystem,
    service::{EmbedLocker, NodeMetadata, Storage, VectorOutcome},
};
use serde_json::{Value, json};
use std::process::Output;
use std::{fs, process::Command};
use support::{
    embedding::{FixtureReply, HttpFixture},
    provenance::{build_note, create_basis},
};
use support::{git::isolate_command, reading::ReadingRepo};

#[test]
fn reindex_binary_rebuilds_unchanged_fts_and_returns_next_revision() {
    // Given: an unchanged knowledge file whose FTS entry was removed.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    assert_eq!(reindex(&repo), json!({"ok":true,"result":{"revision":1}}));
    let db = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    db.execute("delete from text_index", []).unwrap();
    // When: the reindex command rebuilds the cache.
    assert_eq!(reindex(&repo), json!({"ok":true,"result":{"revision":2}}));
    // Then: the next revision restores the original searchable text.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(
        read.get_text_matches_by_expression("AlphaToken").unwrap()[0]
            .id
            .as_str(),
        "a.md"
    );
    drop(read);
    assert_eq!(reindex(&repo), json!({"ok":true,"result":{"revision":3}}));
}

#[test]
fn reindex_binary_rejects_selective_cancel_and_malformed_input_before_io() {
    // Given.
    let repo = ReadingRepo::new();
    for (args, input) in [
        (vec!["reindex"], r#"{"paths":["a.md"]}"#),
        (vec!["reindex", "--cancel"], "{}"),
        (vec!["reindex"], "null"),
        (vec!["reindex"], "{} {}"),
        (vec!["reindex"], r#"{"extra":1}"#),
    ] {
        // When.
        let response = get_response(&repo.invoke(&args, input.as_bytes()), 1);
        // Then.
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(!repo.get_root().join(".letopis").exists());
    }
}

#[test]
fn reindex_binary_preserves_invalid_owner_and_exposes_current_diagnostic() {
    // Given: an invalid owner and an independently edited valid peer.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    repo.write_knowledge("b.md", "BravoToken");
    reindex(&repo);
    repo.write("a.md", b"<!-- letopis: SECRET_RAW -->");
    repo.write_knowledge("b.md", "CharlieToken");
    // When: the reindex command publishes the corpus.
    assert_eq!(reindex(&repo)["result"]["revision"], 2);
    // Then: the invalid owner retains its projection and exposes the current diagnostic.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(
        read.get_text_matches_by_expression("AlphaToken").unwrap()[0]
            .id
            .as_str(),
        "a.md"
    );
    assert_eq!(
        read.get_text_matches_by_expression("CharlieToken").unwrap()[0]
            .id
            .as_str(),
        "b.md"
    );
    assert!(
        read.get_text_matches_by_expression("BravoToken")
            .unwrap()
            .is_empty()
    );
    drop(read);
    let status = get_response(&repo.invoke(&["status"], b"{}"), 0);
    assert_eq!(status["result"]["revision"], 2);
    assert_eq!(status["result"]["problems"][0]["details"]["path"], "a.md");
    assert!(!status.to_string().contains("SECRET_RAW"));
}

#[test]
fn refreshing_cli_detects_external_checkout_and_reset_without_watcher() {
    // Given two committed corpora with different tracked/ignored membership.
    let repo = ReadingRepo::new();
    repo.write(".gitignore", b".letopis/\nignored.md\ntracked.md\n");
    repo.write_knowledge("old.md", "OldToken SharedToken");
    repo.write_knowledge("tracked.md", "TrackedToken SharedToken");
    repo.write_knowledge("same.md", "FirstVersionToken");
    run_git(&repo, &["add", "old.md", "same.md", ".gitignore"]);
    run_git(&repo, &["add", "-f", "tracked.md"]);
    run_git(&repo, &["commit", "-qm", "first"]);
    let first = run_git(&repo, &["rev-parse", "HEAD"]);
    reindex(&repo);
    run_git(&repo, &["checkout", "-qb", "other"]);
    run_git(&repo, &["rm", "old.md", "tracked.md"]);
    repo.write_knowledge("new.md", "NewToken SharedToken");
    repo.write_knowledge("same.md", "SecondVersionToken");
    run_git(&repo, &["add", "new.md", "same.md"]);
    run_git(&repo, &["commit", "-qm", "second"]);
    repo.write_knowledge("ignored.md", "IgnoredToken SharedToken");
    // When / Then: an ordinary query detects the external branch change.
    assert_eq!(search_ids(&repo, "SharedToken"), ["new.md"]);
    assert_eq!(search_ids(&repo, "SecondVersionToken"), ["same.md"]);
    assert!(search_ids(&repo, "FirstVersionToken").is_empty());
    run_git(&repo, &["reset", "--hard", first.trim()]);
    assert_eq!(search_ids(&repo, "SharedToken"), ["old.md", "tracked.md"]);
}

#[test]
fn reindex_reuses_vectors_without_waiting_for_held_embed_lock() {
    // Given: one cached vector, eligible work and a held lock without any HTTP server.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    repo.write_knowledge("b.md", "BravoToken");
    reindex(&repo);
    let config = repo.write_config("ollama", "http://127.0.0.1:1", "fixture");
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let bytes = fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap();
    let files = open_filesystem(repo.get_root()).unwrap();
    let _lock = files.try_lock_embed().unwrap().unwrap();
    // When: cache deletion must leave vector data and configuration alone.
    assert_eq!(reindex(&repo)["result"]["revision"], 2);
    fs::remove_file(repo.get_root().join(".letopis/cache.db")).unwrap();
    assert_eq!(
        get_response(&repo.invoke(&["status"], b"{}"), 0)["result"]["revision"],
        Value::Null
    );
    assert!(!repo.get_root().join(".letopis/cache.db").exists());
    assert_eq!(reindex(&repo)["result"]["revision"], 1);
    // Then: vectors are reused while configuration and the held embedding lock survive.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    let node = read
        .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!()
    };
    assert_eq!(
        read.get_vector_by_search_hash(&config.get_identity(), &node.search_hash)
            .unwrap(),
        VectorOutcome::Current(Some(vec![1.0, 0.0]))
    );
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap(),
        bytes
    );
}

#[test]
fn reindex_reuses_vectors_and_only_requests_the_missing_hash() {
    // Given: publish two hashes, seed exactly one vector, and close storage handles.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    repo.write_knowledge("b.md", "BravoToken");
    reindex(&repo);
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[0,1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0, 0.0])]);
    let files = open_filesystem(repo.get_root()).unwrap();
    let lock = files.try_lock_embed().unwrap().unwrap();
    // When: rebuild only the disposable cache while automatic embedding is excluded.
    fs::remove_file(repo.get_root().join(".letopis/cache.db")).unwrap();
    assert_eq!(reindex(&repo)["result"]["revision"], 1);
    assert!(http.take_requests().is_empty());
    drop(lock);
    assert_eq!(
        get_response(&repo.invoke(&["embed", "--drain"], b""), 0),
        json!({"ok":true,"result":{}})
    );
    // Then: no recomputation of a.md; its exact seed and the new vector both survive.
    assert_eq!(
        http.take_requests(),
        vec![json!({
            "model":"fixture", "input":["BravoToken"], "truncate":false
        })]
    );
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    for (id, vector) in [("a.md", vec![1., 0., 0.]), ("b.md", vec![0., 1., 0.])] {
        let node = read
            .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
            .unwrap()
            .unwrap();
        let NodeMetadata::Knowledge(node) = node.metadata else {
            panic!()
        };
        assert_eq!(
            read.get_vector_by_search_hash(&config.get_identity(), &node.search_hash)
                .unwrap(),
            VectorOutcome::Current(Some(vector)),
            "{id}"
        );
    }
}

#[test]
fn reindex_preserves_author_basis_bytes_head_and_index() {
    // Given: an author basis, live source edit and captured Git state.
    let repo = ReadingRepo::new();
    let tree = create_basis(repo.get_root(), &[("source.txt", b"old source")]);
    repo.write("source.txt", b"new source");
    let note = build_note(Some(&tree), &["source.txt"], "AlphaToken");
    repo.write("a.md", note.as_bytes());
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    let index = fs::read(repo.get_root().join(".git/index")).unwrap();
    // When: reindex runs twice.
    reindex(&repo);
    reindex(&repo);
    // Then: author provenance, canonical bytes, HEAD and index remain intact.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    let node = read
        .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!()
    };
    assert_eq!(node.source_snapshot.as_ref(), Some(&tree));
    assert_eq!(
        fs::read(repo.get_root().join("a.md")).unwrap(),
        note.as_bytes()
    );
    assert_eq!(
        fs::read(repo.get_root().join("source.txt")).unwrap(),
        b"new source"
    );
    assert_eq!(fs::read(repo.get_root().join(".git/index")).unwrap(), index);
    assert_eq!(run_git(&repo, &["rev-parse", "HEAD"]), head);
}

#[test]
fn reindex_reports_safe_capture_failure_and_keeps_previous_revision() {
    // Given: a prior publication and an unsafe symlink candidate.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    reindex(&repo);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.md"), "SECRET_OUTSIDE").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.md"),
        repo.get_root().join("unsafe.md"),
    )
    .unwrap();
    repo.write_knowledge("a.md", "UnpublishedToken");
    // When: the reindex command encounters the unsafe candidate.
    let response = get_response(&repo.invoke(&["reindex"], b"{}"), 1);
    // Then: a safe error is returned and the prior revision survives.
    assert_eq!(
        response,
        json!({"ok":false,"error":{"code":"invalid_request","message":"cannot rebuild corpus index","details":{}}})
    );
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(
        read.get_text_matches_by_expression("AlphaToken").unwrap()[0]
            .id
            .as_str(),
        "a.md"
    );
    assert!(
        read.get_text_matches_by_expression("UnpublishedToken")
            .unwrap()
            .is_empty()
    );
}

fn reindex(repo: &ReadingRepo) -> Value {
    let response = get_response(&repo.invoke(&["reindex"], b"{}"), 0);
    println!("PRODUCT_REINDEX {}", response);
    response
}
fn search_ids(repo: &ReadingRepo, query: &str) -> Vec<String> {
    let input = json!({"query":query,"limit":10}).to_string();
    let response = get_response(&repo.invoke(&["search"], input.as_bytes()), 0);
    let mut ids: Vec<String> = response["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["element_id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}
fn run_git(repo: &ReadingRepo, args: &[&str]) -> String {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn get_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    serde_json::from_slice(&output.stdout).unwrap()
}
