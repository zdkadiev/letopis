//! Product-only denial/tracing intervals; Git fixtures and oracles run outside them.
mod support;

use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    service::{NodeMetadata, Storage, StorageRead, VectorOutcome},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Seek, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::{
    embedding::{FixtureReply, HttpFixture},
    git::{FORMATS, OwnedChild, TempRepo, isolate_command},
};
use tempfile::TempDir;

const NOTE: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nNeedle\n[source](source.txt \"letopis:type=external;path=source.txt\")\n";
struct Denial {
    directory: TempDir,
}

#[test]
fn sparse_index_inspect_captures_unrequested_tracked_ignored_members_without_helpers() {
    for format in FORMATS {
        // Given: both formats contain a real collapsed index and staged membership absent from HEAD.
        let repo = TempRepo::new(format);
        repo.prepare_sparse_index();
        let before = repo.capture_fixture_state();
        let restored = fs::read(repo.get_root().join("omit/staged.md")).unwrap();
        let denial = Denial::new();
        // When: a request addresses only a file inside the sparse cone.
        let result = denial.success(
            repo.get_root(),
            &["inspect"],
            r#"{"element_ids":["keep/a.md"]}"#,
        );
        // Then: Git independently enumerates complete marked membership, preserving user state.
        let tree = result["snapshot"].as_str().unwrap();
        assert_eq!(
            repo.run_git_successfully(&["ls-tree", "-r", "--name-only", tree], None),
            b"keep/a.md\nomit/b.md\nomit/nested/c.markdown\nomit/staged.md\n"
        );
        assert_eq!(
            repo.run_git_successfully(&["show", &format!("{tree}:omit/staged.md")], None),
            restored
        );
        assert_eq!(repo.capture_fixture_state(), before);
        assert_eq!(
            fs::read(repo.get_root().join("omit/staged.md")).unwrap(),
            restored
        );
        assert!(!repo.get_root().join("omit/deleted.md").exists());
    }
}

#[test]
fn model_free_commands_use_only_local_library_git() {
    for format in FORMATS {
        // Given: real Git prepares each format before any denied/traced product call.
        let repo = TempRepo::new(format);
        prepare_note(&repo);
        let denial = Denial::new();
        // When / Then: commands preserve independently specified literal outcomes.
        assert_eq!(denial.success(repo.get_root(), &["init"], "{}"), json!({}));
        assert_eq!(
            denial.success(repo.get_root(), &["check"], "{}"),
            json!({"errors":[]})
        );
        let config = denial.success(repo.get_root(), &["config"], "{}");
        assert_eq!(config["embedding_model"]["enabled"], false);
        let status = denial.success(repo.get_root(), &["status"], "{}");
        assert_eq!(status["revision"], 1);
        let search = denial.success(
            repo.get_root(),
            &["search"],
            r#"{"query":"Needle","limit":5}"#,
        );
        assert_eq!(search.as_array().unwrap().len(), 1);
        assert_eq!(search[0]["element_id"], "note.md");
        let inspected = denial.success(
            repo.get_root(),
            &["inspect"],
            r#"{"element_ids":["note.md","source.txt"]}"#,
        );
        assert_eq!(inspected["elements"]["source.txt"]["path"], "source.txt");
        assert_eq!(
            inspected["elements"]["source.txt"]["hash"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let traversed = denial.success(repo.get_root(), &["traverse"], r#"{"element_ids":["note.md"],"mode":"relations","direction":"outgoing","depth":1,"limit":5}"#);
        assert_eq!(traversed["elements"][0]["element_id"], "source.txt");
        assert_eq!(
            denial.success(repo.get_root(), &["reindex"], "{}"),
            json!({"revision":2})
        );
        assert_eq!(
            denial.success(repo.get_root(), &["embed", "--drain"], ""),
            json!({})
        );
        fs::write(
            repo.get_root().join("note.md"),
            b"<!-- letopis: {\"created_at\":1} -->\nUpdated\n",
        )
        .unwrap();
        let refreshed = denial.success(
            repo.get_root(),
            &["search"],
            r#"{"query":"Updated","limit":5}"#,
        );
        assert_eq!(refreshed[0]["element_id"], "note.md");
        denial.assert_no_trap();
    }
}

#[test]
fn error_paths_never_fall_back_to_executable_git() {
    for format in FORMATS {
        // Given: valid product state with one retained reading tree.
        let repo = TempRepo::new(format);
        fs::write(repo.get_root().join("note.md"), NOTE).unwrap();
        let denial = Denial::new();
        denial.success(repo.get_root(), &["init"], "{}");
        let captured = denial.success(
            repo.get_root(),
            &["inspect"],
            r#"{"element_ids":["note.md"]}"#,
        );
        let oid = captured["snapshot"].as_str().unwrap();
        let object = repo
            .get_root()
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..]);
        fs::remove_file(&object).unwrap();
        fs::write(object, b"corrupted private bytes").unwrap();
        // When / Then: independent expected errors, without accepting partial success.
        let query = json!({"query":"Needle","limit":5,"filters":{"source_revisions":[oid]}});
        denial.failure(
            repo.get_root(),
            &["search"],
            &query.to_string(),
            "internal_error",
        );
        denial.failure(
            repo.get_root(),
            &["inspect"],
            r#"{"element_ids":["note.md","missing.md"]}"#,
            "not_found",
        );
        fs::write(repo.get_root().join(".git/objects/info/alternates"), b"").unwrap();
        denial.failure(repo.get_root(), &["check"], "{}", "internal_error");
        fs::remove_file(repo.get_root().join(".git/objects/info/alternates")).unwrap();
        fs::write(
            repo.get_root().join(".letopis/config.json"),
            b"malformed private config",
        )
        .unwrap();
        denial.failure(repo.get_root(), &["config"], "{}", "embedding_failed");
        denial.assert_no_trap();
    }
}

#[test]
fn help_version_and_invalid_roots_need_no_git_executable() {
    // Given: no repository, and an independently prepared bare repository.
    let outside = tempfile::tempdir().unwrap();
    let bare = tempfile::tempdir().unwrap();
    let result = Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(bare.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    let denial = Denial::new();
    // When / Then: text help/version succeed outside a repository; init rejects both roots.
    for args in [["--help"], ["--version"]] {
        let output = denial.invoke(outside.path(), &args, "");
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("letopis")
        );
    }
    for root in [outside.path(), bare.path()] {
        denial.failure(root, &["init"], "{}", "invalid_request");
        assert!(!root.join(".letopis").exists());
    }
    denial.assert_no_trap();
}

#[test]
fn nonempty_direct_drain_uses_owned_local_provider_without_git_helpers() {
    // Given: publish with disabled embeddings, then configure only an owned fake server.
    let repo = support::reading::ReadingRepo::new();
    repo.write_knowledge("note.md", "Needle");
    let denial = Denial::new();
    denial.success(repo.get_root(), &["init"], "{}");
    denial.success(repo.get_root(), &["check"], "{}");
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,2,3]]}"#.into(),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    // When: direct drain processes actual nonempty pending work under denial/tracing.
    assert_eq!(
        denial.success(repo.get_root(), &["embed", "--drain"], ""),
        json!({})
    );
    // Then: exactly one local request stores the independent literal vector.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    let work = read
        .get_next_embedding_work_by_id_and_max_attempts(
            &config.get_identity(),
            config.get_max_attempts(),
            None,
        )
        .unwrap();
    assert_eq!(work, VectorOutcome::Current(None));
    let id = EdgeTarget::Knowledge(KnowledgeId::parse("note.md").unwrap());
    let node = read.get_node_by_id(&id).unwrap().unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!("knowledge node")
    };
    assert_eq!(
        read.get_vector_by_search_hash(&config.get_identity(), &node.search_hash)
            .unwrap(),
        VectorOutcome::Current(Some(vec![1., 2., 3.]))
    );
    assert_eq!(http.take_requests().len(), 1);
    denial.assert_no_trap();
}

impl Denial {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("git");
        fs::write(
            &executable,
            b"#!/bin/sh\nprintf 'attempt\\n' >> \"$LETOPIS_GIT_TRAP\"\nexit 97\n",
        )
        .unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self { directory }
    }

    fn success(&self, root: &Path, args: &[&str], input: &str) -> Value {
        let output = self.invoke(root, args, input);
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], true, "{value}");
        value["result"].clone()
    }

    fn failure(&self, root: &Path, args: &[&str], input: &str, code: &str) {
        let output = self.invoke(root, args, input);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], code, "{value}");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("private")
        );
    }

    fn invoke(&self, root: &Path, args: &[&str], input: &str) -> Output {
        let trace = tempfile::tempdir().unwrap();
        let binary = fs::canonicalize(env!("CARGO_BIN_EXE_letopis")).unwrap();
        let mut command = self.build_command(&binary, trace.path());
        isolate_command(&mut command);
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
        stdin.rewind().unwrap();
        command
            .current_dir(root)
            .args(args)
            .env("PATH", self.directory.path())
            .env("LETOPIS_GIT_TRAP", self.directory.path().join("attempts"))
            .env_remove("OPENAI_API_KEY")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30));
        assert!(output.stderr.is_empty(), "{output:?}");
        self.assert_no_trap();
        if std::env::var_os("LETOPIS_TEST_STRACE").is_some() {
            assert_trace(trace.path(), &binary);
            if let Some(evidence) = std::env::var_os("LETOPIS_TEST_EVIDENCE") {
                let target = Path::new(&evidence).join("git-traces");
                fs::create_dir_all(&target).unwrap();
                let saved = tempfile::Builder::new()
                    .prefix("invocation-")
                    .tempdir_in(target)
                    .unwrap();
                for entry in fs::read_dir(trace.path()).unwrap() {
                    let entry = entry.unwrap();
                    fs::copy(entry.path(), saved.path().join(entry.file_name())).unwrap();
                }
                fs::write(
                    saved.path().join("argv.json"),
                    serde_json::to_vec(args).unwrap(),
                )
                .unwrap();
                let _ = saved.keep();
            }
        }
        output
    }

    fn build_command(&self, binary: &Path, trace: &Path) -> Command {
        let Some(strace) = std::env::var_os("LETOPIS_TEST_STRACE") else {
            return Command::new(binary);
        };
        assert!(Path::new(&strace).is_absolute());
        let mut command = Command::new(strace);
        command
            .args(["-ff", "-qq", "-e", "trace=process", "-o"])
            .arg(trace.join("process"))
            .arg(binary);
        command
    }

    fn assert_no_trap(&self) {
        assert!(
            !self.directory.path().join("attempts").exists(),
            "product attempted executable Git"
        );
    }
}

fn assert_trace(directory: &Path, binary: &Path) {
    let mut executions = 0;
    let expected = format!("execve({:?},", binary.to_str().unwrap());
    let paths: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(!paths.is_empty(), "strace produced no evidence");
    for path in paths {
        let trace = fs::read_to_string(path).unwrap();
        for line in trace.lines() {
            assert!(
                !line.starts_with("fork(") && !line.starts_with("vfork("),
                "unexpected fork: {line}"
            );
            if line.starts_with("clone(") || line.starts_with("clone3(") {
                assert!(
                    line.contains("CLONE_THREAD"),
                    "unexpected non-thread clone: {line}"
                );
            }
        }
        for line in trace.lines().filter(|line| line.contains("execve")) {
            assert!(
                line.starts_with(&expected),
                "unexpected product descendant execution: {line}"
            );
            executions += 1;
        }
    }
    assert_eq!(
        executions, 1,
        "this matrix has no eligible detached work at foreground exit"
    );
    println!("TRACE_ONLY_LETOPIS {}", binary.display());
}

fn prepare_note(repo: &TempRepo) {
    fs::write(repo.get_root().join("source.txt"), b"abc").unwrap();
    repo.run_git_successfully(&["add", "source.txt"], None);
    let tree = repo.get_oid(&["write-tree"], None);
    let note = String::from_utf8(NOTE.to_vec()).unwrap().replace(
        "\"created_at\":1",
        &format!("\"created_at\":1,\"source_snapshot\":\"{tree}\""),
    );
    fs::write(repo.get_root().join("note.md"), note).unwrap();
}
