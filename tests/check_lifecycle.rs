mod support;

use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    service::{Checker, KnowledgeMetadata, NodeMetadata, Storage, StorageRead},
    wire_checker,
};
use serde_json::{Value, json};
use std::{fs, process::Command, sync::Barrier, thread};
use support::{documents::build_whole, git::isolate_command, reading::ReadingRepo};

#[test]
fn check_tracks_file_and_directory_rename_without_touching_sources() {
    // Arrange: the target is tracked, while the source keeps its original literal link.
    let repo = ReadingRepo::new();
    let source = build_whole(7, "[target](knowledge/old.md \"letopis:type=supports\")");
    repo.write("source.md", source.as_bytes());
    repo.write(
        "knowledge/old.md",
        build_whole(42, "Original target").as_bytes(),
    );
    track_target(&repo, "knowledge/old.md");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // Act / Assert: a file rename changes the ID without rewriting the declaration.
    rename_path(&repo, "knowledge/old.md", "knowledge/new.md");
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    let moved = read_knowledge(&repo, "knowledge/new.md").unwrap();
    assert_eq!(moved.created_at, 42);
    assert!(read_knowledge(&repo, "knowledge/old.md").is_none());
    rename_path(&repo, "knowledge/new.md", "knowledge/old.md");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // Act / Assert: deletion and restoration use the still-tracked missing path.
    fs::remove_file(repo.get_root().join("knowledge/old.md")).unwrap();
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    assert!(read_knowledge(&repo, "knowledge/old.md").is_none());
    repo.write(
        "knowledge/old.md",
        build_whole(42, "Restored target").as_bytes(),
    );
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // Act / Assert: moving a containing directory has the same literal-ID semantics.
    rename_path(&repo, "knowledge", "renamed");
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    assert_eq!(
        read_knowledge(&repo, "renamed/old.md").unwrap().created_at,
        42
    );
    assert!(read_knowledge(&repo, "knowledge/old.md").is_none());
    rename_path(&repo, "renamed", "knowledge");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());
    let restored = read_knowledge(&repo, "knowledge/old.md").unwrap();
    assert_eq!(restored.created_at, 42);
}

#[test]
fn check_tracks_anchor_rename_removal_and_repair() {
    let repo = ReadingRepo::new();
    let source = build_whole(7, "[target](target.md#old \"letopis:type=supports\")");
    repo.write("source.md", source.as_bytes());
    repo.write("target.md", build_section("old").as_bytes());
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    repo.write("target.md", build_section("new").as_bytes());
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    assert_eq!(
        read_knowledge(&repo, "target.md#new").unwrap().created_at,
        42
    );
    assert!(read_knowledge(&repo, "target.md#old").is_none());
    repo.write("target.md", b"Ordinary Markdown without an element\n");
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    assert!(read_knowledge(&repo, "target.md#new").is_none());
    repo.write("target.md", build_section("old").as_bytes());
    assert_eq!(check_preserving_source(&repo, &source), build_clean());
    assert_eq!(
        read_knowledge(&repo, "target.md#old").unwrap().created_at,
        42
    );

    repo.write("target.md", build_section("new").as_bytes());
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    let repaired = build_whole(7, "[target](target.md#new \"letopis:type=supports\")");
    repo.write("source.md", repaired.as_bytes());
    assert_eq!(check_preserving_source(&repo, &repaired), build_clean());
    assert_eq!(
        read_knowledge(&repo, "target.md#new").unwrap().created_at,
        42
    );
}

#[test]
fn check_keeps_last_valid_projection_while_reporting_current_file_error() {
    let repo = ReadingRepo::new();
    repo.write(
        "retained.md",
        build_whole(42, "RetainedOriginal").as_bytes(),
    );
    assert_eq!(check(&repo), build_clean());
    let original = read_knowledge(&repo, "retained.md").unwrap();
    repo.write("retained.md", b"<!-- letopis: broken PRIVATE_BODY -->\n");

    let response = check(&repo);

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"invalid letopis metadata",
             "details":{"path":"retained.md","reason":"invalid_metadata"}}
        ]}})
    );
    assert_eq!(read_knowledge(&repo, "retained.md").unwrap(), original);
    repo.write("retained.md", build_whole(42, "RepairedCurrent").as_bytes());
    assert_eq!(check(&repo), build_clean());
    let repaired = read_knowledge(&repo, "retained.md").unwrap();
    assert_eq!(repaired.created_at, 42);
    assert_ne!(repaired.hash, original.hash);
    let mut storage = repo.open_storage();
    let mut view = storage.begin_read().unwrap();
    assert!(view.match_text("RetainedOriginal").unwrap().is_empty());
    assert_eq!(
        view.match_text("RepairedCurrent").unwrap()[0].id.as_str(),
        "retained.md"
    );
}

#[test]
fn concurrent_check_publishes_edits_from_both_writers() {
    let repo = ReadingRepo::new();
    repo.write("a.md", build_whole(42, "BeforeA").as_bytes());
    repo.write("b.md", build_whole(73, "BeforeB").as_bytes());
    assert_eq!(check(&repo), build_clean());
    let barrier = Barrier::new(2);

    thread::scope(|scope| {
        let first = scope.spawn(|| {
            let mut checker = wire_checker(repo.get_root()).unwrap();
            repo.write("a.md", build_whole(42, "LarchEdited").as_bytes());
            barrier.wait();
            checker.check(&[]).unwrap()
        });
        let second = scope.spawn(|| {
            let mut checker = wire_checker(repo.get_root()).unwrap();
            repo.write("b.md", build_whole(73, "BirchEdited").as_bytes());
            barrier.wait();
            checker.check(&[]).unwrap()
        });
        assert!(first.join().unwrap().errors.is_empty());
        assert!(second.join().unwrap().errors.is_empty());
    });

    // One final pinned view independently observes both IDs, authored times and FTS texts.
    let mut storage = repo.open_storage();
    let mut view = storage.begin_read().unwrap();
    let nodes = view.list_nodes().unwrap();
    assert_eq!(nodes.len(), 2);
    for (id, created_at, text) in [("a.md", 42, "LarchEdited"), ("b.md", 73, "BirchEdited")] {
        let node = view.read_node(&build_target(id)).unwrap().unwrap();
        let NodeMetadata::Knowledge(metadata) = node.metadata else {
            panic!("knowledge node expected")
        };
        assert_eq!(metadata.id.as_str(), id);
        assert_eq!(metadata.created_at, created_at);
        let matches = view.match_text(text).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id.as_str(), id);
    }
}

#[test]
fn repeated_check_preserves_corpus_and_revision() {
    let repo = ReadingRepo::new();
    repo.write(
        "source.md",
        build_whole(42, "[absent](missing.md \"letopis:type=supports\")").as_bytes(),
    );
    repo.write("ordinary.md", b"Unmarked Markdown\n");
    let before =
        ["source.md", "ordinary.md"].map(|path| fs::read(repo.get_root().join(path)).unwrap());
    let first = check(&repo);
    assert_eq!(first, build_dangling("missing.md"));
    let mut storage = repo.open_storage();
    let revision = storage.begin_read().unwrap().get_revision();

    for _ in 0..3 {
        assert_eq!(check(&repo), first);
        assert_eq!(storage.begin_read().unwrap().get_revision(), revision);
        for (path, bytes) in ["source.md", "ordinary.md"].into_iter().zip(&before) {
            assert_eq!(fs::read(repo.get_root().join(path)).unwrap(), *bytes);
        }
    }
    assert!(!repo.get_root().join("missing.md").exists());
    assert_eq!(read_knowledge(&repo, "source.md").unwrap().created_at, 42);
}

fn rename_path(repo: &ReadingRepo, from: &str, to: &str) {
    fs::rename(repo.get_root().join(from), repo.get_root().join(to)).unwrap();
}

fn track_target(repo: &ReadingRepo, path: &str) {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(["add", "--", path])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn check_preserving_source(repo: &ReadingRepo, source: &str) -> Value {
    let response = check(repo);
    assert_eq!(
        fs::read(repo.get_root().join("source.md")).unwrap(),
        source.as_bytes()
    );
    response
}

fn check(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["check"], b"{}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn build_clean() -> Value {
    json!({"ok":true,"result":{"errors":[]}})
}

fn build_dangling(target: &str) -> Value {
    json!({"ok":true,"result":{"errors":[
        {"code":"validation_failed","message":"knowledge target is missing",
         "details":{"path":"source.md","reason":"dangling_target","element_id":"source.md","target":target}}
    ]}})
}

fn build_section(anchor: &str) -> String {
    format!("# Target {{#{anchor}}}\n<!-- letopis: {{\"created_at\":42}} -->\nSection content\n")
}

fn read_knowledge(repo: &ReadingRepo, id: &str) -> Option<KnowledgeMetadata> {
    let mut storage = repo.open_storage();
    let row = storage
        .begin_read()
        .unwrap()
        .read_node(&build_target(id))
        .unwrap()?;
    let NodeMetadata::Knowledge(metadata) = row.metadata else {
        panic!("knowledge node expected")
    };
    Some(metadata)
}

fn build_target(id: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
}
