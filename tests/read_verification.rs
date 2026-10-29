mod support;
use letopis::service::{Storage, StorageRead};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Seek, Write},
    os::unix::fs::PermissionsExt,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::{
    git::{OwnedChild, isolate_command},
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

#[test]
fn all_commands_and_freshness_preserve_canonical_bytes() {
    // Given: canonical linked knowledge, local FTS and no provider.
    let repo = ReadingRepo::new();
    assert_ok(&repo.invoke(&["init"], b"{}"));
    repo.write_knowledge(
        "a.md",
        "AlphaToken [peer](b.md \"letopis:type=depends_on\")",
    );
    repo.write_knowledge("b.md", "BravoToken");
    let before = fs::read(repo.get_root().join("a.md")).unwrap();
    // When: every public command is executed through the actual executable.
    assert_ok(&repo.invoke(&["check"], b"{}"));
    let found = assert_ok(&repo.invoke(&["search"], br#"{"query":"AlphaToken","limit":10}"#));
    let walk = assert_ok(&repo.invoke(&["traverse"], br#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1,"limit":10,"edge_limit":20}"#));
    let inspected = assert_ok(&repo.invoke(&["inspect"], br#"{"element_ids":["a.md"]}"#));
    assert_ok(&repo.invoke(&["reindex"], b"{}"));
    assert_ok(&repo.invoke(&["status"], b"{}"));
    assert_ok(&repo.invoke(&["config"], b"{}"));
    // Then: results and inspection hash describe the untouched canonical file.
    assert_eq!(found["result"][0]["element_id"], "a.md");
    assert_eq!(walk["result"]["elements"][0]["element_id"], "b.md");
    assert_eq!(
        inspected["result"]["elements"]["a.md"]["hash"],
        letopis::domain::Sha256::digest(&before).to_hex()
    );
    assert_eq!(fs::read(repo.get_root().join("a.md")).unwrap(), before);
    // Given: an edit, addition, then deletion without explicit reindex.
    repo.write_knowledge("a.md", "ChangedToken");
    repo.write_knowledge("new.md", "NewToken");
    // When / Then: routine search refreshes both edits and discoveries.
    assert_eq!(
        assert_ok(&repo.invoke(&["search"], br#"{"query":"ChangedToken","limit":10}"#))["result"]
            [0]["element_id"],
        "a.md"
    );
    assert_eq!(
        assert_ok(&repo.invoke(&["search"], br#"{"query":"NewToken","limit":10}"#))["result"][0]["element_id"],
        "new.md"
    );
    fs::remove_file(repo.get_root().join("new.md")).unwrap();
    assert_eq!(
        assert_ok(&repo.invoke(&["search"], br#"{"query":"NewToken","limit":10}"#))["result"],
        json!([])
    );
    for args in [
        ["--help"].as_slice(),
        ["--version"].as_slice(),
        ["check", "--help"].as_slice(),
    ] {
        assert!(repo.invoke(args, b"").status.success());
    }
}

#[test]
fn unrelated_damage_is_deferred_but_full_audits_prevent_publication() {
    // Given: a retained author basis with one used and one unrelated blob.
    let repo = ReadingRepo::new();
    let basis = create_basis(
        repo.get_root(),
        &[("used.rs", b"used\n"), ("unrelated", b"unrelated\n")],
    );
    repo.write("used.rs", b"used\n");
    repo.write(
        "a.md",
        build_note(Some(&basis), &["used.rs"], "AlphaToken").as_bytes(),
    );
    assert_ok(&repo.invoke(&["check"], b"{}"));
    let revision = get_revision(&repo);
    damage_child(&repo, basis.as_str(), "unrelated");
    // When / Then: routine reads do not touch unrelated author bytes.
    assert_no_source_audit(&repo, "search", br#"{"query":"AlphaToken","limit":10}"#, 0);
    assert_no_source_audit(
        &repo,
        "traverse",
        br#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1,"limit":10}"#,
        0,
    );
    assert_no_source_audit(
        &repo,
        "inspect",
        br#"{"element_ids":["a.md","used.rs"]}"#,
        1,
    );
    assert_no_source_audit(&repo, "check", br#"{"paths":["a.md"]}"#, 0);
    // Given: an unpublished edit to valid knowledge.
    repo.write(
        "a.md",
        build_note(Some(&basis), &["used.rs"], "PendingToken").as_bytes(),
    );
    // When / Then: both whole-corpus audits fail before advancing SQL publication.
    for command in ["check", "reindex"] {
        assert_internal(&repo.invoke(&[command], b"{}"));
        assert_eq!(get_revision(&repo), revision);
    }
    // Given: an invalid edit must keep the last-valid basis, including its damage.
    repo.write("a.md", b"<!-- letopis: invalid -->");
    // When / Then: the old root remains in the prospective valid projection.
    assert_internal(&repo.invoke(&["check"], b"{}"));
    assert_eq!(get_revision(&repo), revision);
    // Given: valid replacement removes the obsolete damaged basis.
    repo.write_knowledge("a.md", "ReplacementToken");
    // When / Then: full audit does not inspect obsolete roots.
    assert_ok(&repo.invoke(&["check"], b"{}"));
    assert_eq!(get_revision(&repo), revision + 1);
}

#[test]
fn newly_declared_bases_and_markdown_edits_never_trigger_source_audit() {
    // Given: a new author root is damaged before its first routine retention.
    let repo = ReadingRepo::new();
    let basis = create_basis(
        repo.get_root(),
        &[("used.rs", b"used\n"), ("unrelated", b"unrelated\n")],
    );
    damage_child(&repo, basis.as_str(), "unrelated");
    repo.write("used.rs", b"used\n");
    repo.write(
        "a.md",
        build_note(Some(&basis), &["used.rs"], "AlphaToken").as_bytes(),
    );
    // When / Then: first publication retains only the root and addressed bytes.
    assert_no_source_audit(&repo, "search", br#"{"query":"AlphaToken","limit":10}"#, 0);
    let retained = run_git(
        &repo,
        &[
            "rev-parse",
            &format!("refs/letopis/snapshots/{}", basis.as_str()),
        ],
    );
    assert_eq!(String::from_utf8(retained).unwrap().trim(), basis.as_str());
    // Given: a normal Markdown edit retaining the same basis.
    repo.write(
        "a.md",
        build_note(Some(&basis), &["used.rs"], "ChangedToken").as_bytes(),
    );
    // When / Then: incremental refresh remains light; full audit still detects damage.
    assert_no_source_audit(
        &repo,
        "search",
        br#"{"query":"ChangedToken","limit":10}"#,
        0,
    );
    assert_internal(&repo.invoke(&["check"], b"{}"));
}

#[test]
fn addressed_damage_remains_a_safe_read_failure() {
    // Given: corruption affects the exact historical source bytes being compared.
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("used.rs", b"used\n")]);
    repo.write("used.rs", b"used\n");
    repo.write(
        "a.md",
        build_note(Some(&basis), &["used.rs"], "AlphaToken").as_bytes(),
    );
    assert_ok(&repo.invoke(&["check"], b"{}"));
    damage_child(&repo, basis.as_str(), "used.rs");
    // When / Then: search and scoped check still reject required corrupt bytes.
    assert_internal(&repo.invoke(&["search"], br#"{"query":"AlphaToken","limit":10}"#));
    assert_internal(&repo.invoke(&["check"], br#"{"paths":["a.md"]}"#));
}

#[test]
fn removing_last_owner_drops_obsolete_damaged_root() {
    // Given: one author root is referenced by the sole last-valid owner.
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("unrelated", b"old\n")]);
    repo.write(
        "a.md",
        build_note(Some(&basis), &[], "AlphaToken").as_bytes(),
    );
    assert_ok(&repo.invoke(&["check"], b"{}"));
    damage_child(&repo, basis.as_str(), "unrelated");
    // When: the owner is deleted before the next full audit.
    fs::remove_file(repo.get_root().join("a.md")).unwrap();
    // Then: no surviving claim requires the old damaged root.
    assert_ok(&repo.invoke(&["check"], b"{}"));
    assert_ok(&repo.invoke(&["reindex"], b"{}"));
}

fn assert_ok(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    response
}
fn assert_internal(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "internal_error");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CORRUPTION_CANARY"));
    assert!(output.stderr.is_empty());
}
fn get_revision(repo: &ReadingRepo) -> u64 {
    repo.open_storage().begin_read().unwrap().get_revision()
}
fn damage_child(repo: &ReadingRepo, tree: &str, name: &str) {
    let raw = run_git(repo, &["rev-parse", &format!("{tree}:{name}")]);
    let oid = String::from_utf8(raw).unwrap();
    let oid = oid.trim();
    let path = repo
        .get_root()
        .join(".git/objects")
        .join(&oid[..2])
        .join(&oid[2..]);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(path, b"CORRUPTION_CANARY").unwrap();
}
fn run_git(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let mut cmd = Command::new("git");
    isolate_command(&mut cmd);
    let output = cmd
        .current_dir(repo.get_root())
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}
fn assert_no_source_audit(
    repo: &ReadingRepo,
    command: &str,
    request: &[u8],
    expected_batches: usize,
) {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("git");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$LETOPIS_TEST_CALLS\"\nexec /usr/bin/git \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let log = dir.path().join("calls");
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(request).unwrap();
    input.rewind().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut cmd);
    cmd.current_dir(repo.get_root())
        .arg(command)
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("LETOPIS_TEST_CALLS", &log)
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = OwnedChild::spawn(&mut cmd);
    assert_ok(&child.wait(Instant::now() + Duration::from_secs(30)));
    let calls = fs::read_to_string(log).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|c| c.ends_with("cat-file --batch"))
            .count(),
        expected_batches,
        "{calls}"
    );
}
