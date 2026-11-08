use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead};
use std::num::NonZeroUsize;
mod support;

use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    service::{Checker, KnowledgeMetadata, NodeMetadata},
    wire_checker,
};
use serde_json::{Value, json};
use std::{fs, process::Command, sync::Barrier, thread};
use support::{documents::build_whole, git::isolate_command, reading::ReadingRepo};

#[test]
fn check_tracks_file_and_directory_rename_without_touching_sources() {
    // Given: the target is tracked, while the source keeps its original literal link.
    let repo = ReadingRepo::new();
    let source = build_whole(
        7,
        "[target](knowledge/old.md \"letopis:type=supports;path=knowledge/old.md\")",
    );
    repo.write("source.md", source.as_bytes());
    repo.write(
        "knowledge/old.md",
        build_whole(42, "Original target").as_bytes(),
    );
    track_target(&repo, "knowledge/old.md");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // When / Then: a file rename changes the ID without rewriting the declaration.
    rename_path(&repo, "knowledge/old.md", "knowledge/new.md");
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    let moved = get_knowledge(&repo, "knowledge/new.md").unwrap();
    assert_eq!(moved.created_at, 42);
    assert!(get_knowledge(&repo, "knowledge/old.md").is_none());
    rename_path(&repo, "knowledge/new.md", "knowledge/old.md");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // When / Then: deletion and restoration use the still-tracked missing path.
    fs::remove_file(repo.get_root().join("knowledge/old.md")).unwrap();
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    assert!(get_knowledge(&repo, "knowledge/old.md").is_none());
    repo.write(
        "knowledge/old.md",
        build_whole(42, "Restored target").as_bytes(),
    );
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // When / Then: moving a containing directory has the same literal-ID semantics.
    rename_path(&repo, "knowledge", "renamed");
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("knowledge/old.md")
    );
    assert_eq!(
        get_knowledge(&repo, "renamed/old.md").unwrap().created_at,
        42
    );
    assert!(get_knowledge(&repo, "knowledge/old.md").is_none());
    rename_path(&repo, "renamed", "knowledge");
    assert_eq!(check_preserving_source(&repo, &source), build_clean());
    let restored = get_knowledge(&repo, "knowledge/old.md").unwrap();
    assert_eq!(restored.created_at, 42);
}

#[test]
fn check_tracks_anchor_rename_removal_and_repair() {
    // Given
    let repo = ReadingRepo::new();
    let source = build_whole(
        7,
        "[target](target.md#old \"letopis:type=supports;path=target.md#old\")",
    );
    repo.write("source.md", source.as_bytes());
    // Given
    repo.write("target.md", build_section("old").as_bytes());
    // When / Then
    assert_eq!(check_preserving_source(&repo, &source), build_clean());

    // Given
    repo.write("target.md", build_section("new").as_bytes());
    // When / Then
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    assert_eq!(
        get_knowledge(&repo, "target.md#new").unwrap().created_at,
        42
    );
    assert!(get_knowledge(&repo, "target.md#old").is_none());
    // Given
    repo.write("target.md", b"Ordinary Markdown without an element\n");
    // When / Then
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    assert!(get_knowledge(&repo, "target.md#new").is_none());
    // Given
    repo.write("target.md", build_section("old").as_bytes());
    // When / Then
    assert_eq!(check_preserving_source(&repo, &source), build_clean());
    assert_eq!(
        get_knowledge(&repo, "target.md#old").unwrap().created_at,
        42
    );

    // Given
    repo.write("target.md", build_section("new").as_bytes());
    // When / Then
    assert_eq!(
        check_preserving_source(&repo, &source),
        build_dangling("target.md#old")
    );
    let repaired = build_whole(
        7,
        "[target](target.md#new \"letopis:type=supports;path=target.md#new\")",
    );
    // Given
    repo.write("source.md", repaired.as_bytes());
    // When / Then
    assert_eq!(check_preserving_source(&repo, &repaired), build_clean());
    assert_eq!(
        get_knowledge(&repo, "target.md#new").unwrap().created_at,
        42
    );
}

#[test]
fn check_keeps_last_valid_projection_while_reporting_current_file_error() {
    // Given: a valid knowledge file before invalid metadata and a later repair.
    let repo = ReadingRepo::new();
    repo.write(
        "retained.md",
        build_whole(42, "RetainedOriginal").as_bytes(),
    );
    // When / Then: check publishes valid contents and retains them through invalid edits.
    assert_eq!(check(&repo), build_clean());
    let original = get_knowledge(&repo, "retained.md").unwrap();
    repo.write("retained.md", b"<!-- letopis: broken PRIVATE_BODY -->\n");

    let response = check(&repo);

    assert_eq!(
        response,
        json!({"ok":true,"result":{"errors":[
            {"code":"validation_failed","message":"invalid letopis metadata",
             "details":{"path":"retained.md","reason":"invalid_metadata"}}
        ]}})
    );
    assert_eq!(get_knowledge(&repo, "retained.md").unwrap(), original);
    repo.write("retained.md", build_whole(42, "RepairedCurrent").as_bytes());
    assert_eq!(check(&repo), build_clean());
    let repaired = get_knowledge(&repo, "retained.md").unwrap();
    assert_eq!(repaired.created_at, 42);
    assert_ne!(repaired.hash, original.hash);
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            let view = &storage.cache;
            assert!(
                view.text_index_get_by_expression(
                    "RetainedOriginal",
                    NonZeroUsize::new(100).unwrap()
                )
                .unwrap()
                .is_empty()
            );
            assert_eq!(
                view.text_index_get_by_expression(
                    "RepairedCurrent",
                    NonZeroUsize::new(100).unwrap()
                )
                .unwrap()[0]
                    .id
                    .as_str(),
                "retained.md"
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn concurrent_check_publishes_edits_from_both_writers() {
    // Given: two published files edited by separate synchronized checkers.
    let repo = ReadingRepo::new();
    repo.write("a.md", build_whole(42, "BeforeA").as_bytes());
    repo.write("b.md", build_whole(73, "BeforeB").as_bytes());
    // When / Then: the initial check publishes both valid files.
    assert_eq!(check(&repo), build_clean());
    let barrier = Barrier::new(2);

    // When: synchronized checkers publish independent edits.
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
        // Then: both checkers complete without findings.
        assert!(first.join().unwrap().errors.is_empty());
        assert!(second.join().unwrap().errors.is_empty());
    });

    // Then: one pinned view observes both IDs, authored times and FTS texts.
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            let view = &storage.cache;
            let nodes = view.nodes_get().unwrap();
            assert_eq!(nodes.len(), 2);
            for (id, created_at, text) in [("a.md", 42, "LarchEdited"), ("b.md", 73, "BirchEdited")]
            {
                let node = StorageRead::nodes_get_by_uri(view, &build_target(id))
                    .unwrap()
                    .unwrap();
                let NodeMetadata::Knowledge(metadata) = node.metadata else {
                    panic!("knowledge node expected")
                };
                assert_eq!(metadata.id.as_str(), id);
                assert_eq!(metadata.created_at, created_at);
                let matches = view
                    .text_index_get_by_expression(text, NonZeroUsize::new(100).unwrap())
                    .unwrap();
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].id.as_str(), id);
            }
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn repeated_check_preserves_corpus_and_revision() {
    // Given: an unchanged published corpus.
    let repo = ReadingRepo::new();
    repo.write(
        "source.md",
        build_whole(
            42,
            "[absent](missing.md \"letopis:type=supports;path=missing.md\")",
        )
        .as_bytes(),
    );
    repo.write("ordinary.md", b"Unmarked Markdown\n");
    let before =
        ["source.md", "ordinary.md"].map(|path| fs::read(repo.get_root().join(path)).unwrap());
    // When: check runs repeatedly.
    let first = check(&repo);
    // Then: canonical bytes and revision remain unchanged.
    assert_eq!(first, build_dangling("missing.md"));
    let storage = repo.open_storage();
    let revision = storage
        .cache
        .with_read_tx(|| storage.cache.project_get_by_id())
        .unwrap();

    for _ in 0..3 {
        assert_eq!(check(&repo), first);
        assert_eq!(
            storage
                .cache
                .with_read_tx(|| storage.cache.project_get_by_id())
                .unwrap(),
            revision
        );
        for (path, bytes) in ["source.md", "ordinary.md"].into_iter().zip(&before) {
            assert_eq!(fs::read(repo.get_root().join(path)).unwrap(), *bytes);
        }
    }
    assert!(!repo.get_root().join("missing.md").exists());
    assert_eq!(get_knowledge(&repo, "source.md").unwrap().created_at, 42);
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

fn get_knowledge(repo: &ReadingRepo, id: &str) -> Option<KnowledgeMetadata> {
    let storage = repo.open_storage();
    let row = storage
        .cache
        .with_read_tx(|| {
            Ok::<_, StorageError>(StorageRead::nodes_get_by_uri(
                &storage.cache,
                &build_target(id),
            ))
        })
        .unwrap()
        .unwrap()?;
    let NodeMetadata::Knowledge(metadata) = row.metadata else {
        panic!("knowledge node expected")
    };
    Some(metadata)
}

fn build_target(id: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
}
