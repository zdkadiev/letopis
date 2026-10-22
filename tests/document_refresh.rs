mod support;
use letopis::{
    domain::{ElementKind, LineRange, Sha256},
    open_documents,
    service::{DocumentErrorKind, Documents, NodeMetadata},
};
use std::{fs, os::unix::fs::symlink};
use support::documents::{DocumentRepo, build_external, build_whole, get_target};

#[test]
fn publish_initial_empty_cache_once() {
    // Arrange: a fresh repository has no service directory.
    let repo = DocumentRepo::new();
    let mut documents = open_documents(repo.get_root()).unwrap();
    // Act.
    let first = documents.refresh().unwrap();
    let second = documents.refresh().unwrap();
    // Assert.
    assert_eq!(first.revision, 1);
    assert!(first.published);
    assert_eq!(second.revision, first.revision);
    assert!(!second.published);
    assert_eq!(
        (
            second.files_updated,
            second.files_removed,
            second.invalid_files_updated
        ),
        (0, 0, 0)
    );
    assert!(repo.read_state().nodes.is_empty());
}

#[test]
fn replace_sections_and_preserve_stable_ids_and_corpus_bytes() {
    let repo = DocumentRepo::new();
    let whole = build_whole(7, "OriginalToken");
    let sections = "# One {#one}\n<!-- letopis: {\"created_at\":8} -->\nFirstToken\n# Two {#two}\n<!-- letopis: {\"created_at\":9} -->\nSecondToken\n";
    repo.write("whole.md", &whole);
    repo.write("under_% score.markdown", sections);
    let mut documents = open_documents(repo.get_root()).unwrap();
    let first = documents.refresh().unwrap();
    let original = repo.read_state();
    assert_eq!(first.files_updated, 2);
    assert_eq!(original.nodes.len(), 3);
    assert_eq!(
        original.get_file("whole.md").hash,
        Sha256::digest(whole.as_bytes())
    );
    // Act: stable identity, different text, and a removed section.
    let edited = build_whole(7, "ReplacementToken");
    repo.write("whole.md", &edited);
    repo.write(
        "under_% score.markdown",
        "# One {#one}\n<!-- letopis: {\"created_at\":8} -->\nChangedSection\n",
    );
    documents.refresh().unwrap();
    let current = repo.read_state();
    assert_eq!(current.nodes.len(), 2);
    for id in ["whole.md", "under_% score.markdown#one"] {
        assert_eq!(original.get_node(id).row_id, current.get_node(id).row_id);
    }
    assert!(repo.search("OriginalToken OR SecondToken").is_empty());
    assert_eq!(repo.search("ReplacementToken"), ["whole.md"]);
    assert_eq!(
        repo.search("ChangedSection"),
        ["under_% score.markdown#one"]
    );
    assert_eq!(
        fs::read(repo.get_root().join("whole.md")).unwrap(),
        edited.as_bytes()
    );
}

#[test]
fn handle_tracked_rename_deletion_and_unmarked_transitions() {
    let repo = DocumentRepo::new();
    repo.write("old.md", build_whole(1, "OldToken"));
    repo.run_git(&["add", "old.md"]);
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    fs::rename(
        repo.get_root().join("old.md"),
        repo.get_root().join("new.md"),
    )
    .unwrap();
    let renamed = documents.refresh().unwrap();
    assert_eq!((renamed.files_updated, renamed.files_removed), (1, 1));
    assert_eq!(repo.search("OldToken"), ["new.md"]);
    assert!(!documents.refresh().unwrap().published);
    repo.write("new.md", "UnmarkedToken\n");
    documents.refresh().unwrap();
    assert!(repo.read_state().nodes.is_empty());
    assert!(repo.read_state().get_file("new.md").valid);
    repo.write("new.md", build_whole(1, "ReintroducedToken"));
    documents.refresh().unwrap();
    assert_eq!(repo.search("ReintroducedToken"), ["new.md"]);
    repo.remove("new.md");
    assert_eq!(documents.refresh().unwrap().files_removed, 1);
    assert!(repo.read_state().files.is_empty());
    assert!(!documents.refresh().unwrap().published);
}

#[test]
fn retain_invalid_projection_while_publishing_valid_peer_and_repair() {
    let repo = DocumentRepo::new();
    repo.write(
        "a.md",
        build_whole(1, "RetainedToken [b](b.md \"letopis:type=includes\")"),
    );
    repo.write("b.md", build_whole(2, "BeforeToken"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    repo.write("a.md", "<!-- letopis: SECRET_RAW_INVALID -->\nBadToken");
    repo.write("b.md", build_whole(2, "AfterToken"));
    repo.write("new-invalid.md", [0xff]);
    let result = documents.refresh().unwrap();
    let invalid = repo.read_state();
    assert_eq!(result.invalid_files_updated, 2);
    assert_eq!(invalid.get_node("a.md"), before.get_node("a.md"));
    assert_eq!(invalid.edges, before.edges);
    assert!(!invalid.get_file("a.md").valid);
    assert!(
        !invalid
            .get_file("a.md")
            .error
            .as_ref()
            .unwrap()
            .contains("SECRET_RAW_INVALID")
    );
    assert!(!invalid.get_file("new-invalid.md").valid);
    assert_eq!(repo.search("RetainedToken"), ["a.md"]);
    assert_eq!(repo.search("AfterToken"), ["b.md"]);
    assert!(repo.search("BeforeToken OR BadToken").is_empty());
    assert!(!documents.refresh().unwrap().published);
    assert_eq!(repo.read_state(), invalid);
    repo.write("a.md", build_whole(1, "RepairedToken"));
    repo.remove("new-invalid.md");
    let repaired = documents.refresh().unwrap();
    assert_eq!(repaired.files_removed, 1);
    assert!(repo.read_state().get_file("a.md").valid);
    assert!(repo.read_state().edges.is_empty());
    assert!(repo.search("RetainedToken").is_empty());
    assert_eq!(repo.search("RepairedToken"), ["a.md"]);
}

#[test]
fn keep_cycles_dangling_declarations_and_incoming_edges_on_target_deletion() {
    let repo = DocumentRepo::new();
    repo.write(
        "a.md",
        build_whole(
            1,
            "[b](b.md \"letopis:type=includes\") [missing](missing.md \"letopis:type=supports\")",
        ),
    );
    repo.write(
        "b.md",
        build_whole(2, "[a](a.md \"letopis:type=includes\")"),
    );
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let state = repo.read_state();
    assert_eq!(state.edges.len(), 3);
    let NodeMetadata::Knowledge(a) = &state.get_node("a.md").metadata else {
        panic!()
    };
    assert_eq!(a.kind, ElementKind::Metavertex);
    repo.remove("b.md");
    documents.refresh().unwrap();
    let state = repo.read_state();
    assert_eq!(state.edges.len(), 2);
    assert!(
        state
            .edges
            .iter()
            .any(|e| e.get_target() == &get_target("b.md", false))
    );
    repo.write("b.md", build_whole(2, "TargetReturns"));
    documents.refresh().unwrap();
    assert_eq!(repo.read_state().edges, state.edges);
}

#[test]
fn reconcile_shared_source_minimum_and_retain_invalid_references() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\r\ntwo\r\nthree\r\nfour");
    repo.write(
        "a.md",
        build_external(3, "source.txt#L02-L03", "EarlyToken"),
    );
    repo.write("b.md", build_external(8, "./source.txt#L2-L3", "LateToken"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    let NodeMetadata::Source(s) = &before.get_node("source.txt#L2-L3").metadata else {
        panic!()
    };
    assert_eq!(s.created_at, 3);
    assert_eq!(s.hash, Some(Sha256::digest(b"two\r\nthree\r\n")));
    assert_eq!(s.lines, Some(LineRange::new(2, 3).unwrap()));
    repo.write("b.md", "<!-- letopis: invalid -->");
    repo.remove("a.md");
    documents.refresh().unwrap();
    let state = repo.read_state();
    let NodeMetadata::Source(s) = &state.get_node("source.txt#L2-L3").metadata else {
        panic!()
    };
    assert_eq!(s.created_at, 8);
    assert_eq!(state.edges.len(), 1);
    repo.remove("b.md");
    documents.refresh().unwrap();
    assert!(repo.read_state().nodes.is_empty());
    assert!(repo.read_state().edges.is_empty());
}

#[test]
fn hash_exact_whole_source_and_represent_missing_or_out_of_range_sources() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"a\r\nb\nc");
    repo.write("a.md", build_external(3, "source.txt", "Whole"));
    repo.write("b.md", build_external(4, "source.txt#L2-L9", "Range"));
    repo.write("c.md", build_external(5, "absent.txt", "Missing"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let state = repo.read_state();
    for (id, hash) in [
        ("source.txt", Some(Sha256::digest(b"a\r\nb\nc"))),
        ("source.txt#L2-L9", None),
        ("absent.txt", None),
    ] {
        let NodeMetadata::Source(s) = &state.get_node(id).metadata else {
            panic!()
        };
        assert_eq!(s.hash, hash);
    }
}

#[test]
fn reject_source_over_existing_knowledge_atomically() {
    let repo = DocumentRepo::new();
    repo.write("a.md", build_whole(1, "OriginalKnowledge"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    repo.write("b.md", build_external(2, "a.md", "NewSourceReference"));
    assert_eq!(
        documents.refresh().unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
    assert_eq!(repo.read_state(), before);
    assert_eq!(repo.search("OriginalKnowledge"), ["a.md"]);
    assert!(repo.search("NewSourceReference").is_empty());
    // The inverse role becomes unambiguous after removing the knowledge projection.
    repo.write("a.md", "source bytes\n");
    documents.refresh().unwrap();
    assert!(matches!(
        repo.read_state().get_node("a.md").metadata,
        NodeMetadata::Source(_)
    ));
    assert!(repo.search("OriginalKnowledge").is_empty());
}

#[test]
fn reject_knowledge_over_referenced_source_and_allow_orphan_role_transition() {
    let repo = DocumentRepo::new();
    repo.write("a.md", "source bytes\n");
    repo.write("z.md", build_external(2, "a.md", "ReferenceToken"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    repo.write("a.md", build_whole(1, "NewKnowledge"));
    assert_eq!(
        documents.refresh().unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
    assert_eq!(repo.read_state(), before);
    assert!(repo.search("NewKnowledge").is_empty());
    repo.write("z.md", build_whole(2, "NoReference"));
    documents.refresh().unwrap();
    let state = repo.read_state();
    assert!(matches!(
        state.get_node("a.md").metadata,
        NodeMetadata::Knowledge(_)
    ));
    assert_eq!(repo.search("NewKnowledge"), ["a.md"]);
    assert!(state.edges.is_empty());
}

#[test]
fn unsafe_source_read_rolls_back_all_cache_changes() {
    let repo = DocumentRepo::new();
    repo.write("a.md", build_whole(1, "PreviousToken"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.txt"), "SECRET_OUTSIDE").unwrap();
    symlink(outside.path(), repo.get_root().join("unsafe")).unwrap();
    repo.write(
        "a.md",
        build_external(1, "unsafe/private.txt", "ChangedToken"),
    );
    let error = documents.refresh().unwrap_err();
    assert!(!format!("{error:?} {error}").contains("SECRET_OUTSIDE"));
    assert_eq!(repo.read_state(), before);
    assert_eq!(repo.search("PreviousToken"), ["a.md"]);
    assert!(repo.search("ChangedToken").is_empty());
}

#[test]
fn recalculate_source_minimum_after_referrer_edit_and_preserve_unaffected_source() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", "OriginalSource");
    repo.write("other.txt", "OtherSource");
    repo.write("a.md", build_external(3, "source.txt", "Early"));
    repo.write("b.md", build_external(8, "source.txt", "Late"));
    repo.write("c.md", build_external(1, "other.txt", "Unchanged"));
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    repo.write("a.md", build_external(10, "source.txt", "Revised"));
    repo.write("other.txt", "UnrelatedNewSourceBytes");
    documents.refresh().unwrap();
    let after = repo.read_state();
    let NodeMetadata::Source(source) = &after.get_node("source.txt").metadata else {
        panic!()
    };
    assert_eq!(source.created_at, 8);
    assert_eq!(after.get_node("other.txt"), before.get_node("other.txt"));
    assert!(!documents.refresh().unwrap().published);
}

#[test]
fn remove_exact_path_without_treating_underscores_or_percent_as_patterns() {
    let repo = DocumentRepo::new();
    for path in ["a_%.md", "aXX.md", "a_%.md.more.md"] {
        repo.write(path, build_whole(1, "SharedToken"));
    }
    let mut documents = open_documents(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before = repo.read_state();
    repo.remove("a_%.md");
    let outcome = documents.refresh().unwrap();
    let after = repo.read_state();
    assert_eq!(outcome.files_removed, 1);
    assert_eq!(after.nodes.len(), 2);
    for path in ["aXX.md", "a_%.md.more.md"] {
        assert_eq!(after.get_node(path), before.get_node(path));
    }
    let mut matches = repo.search("SharedToken");
    matches.sort();
    assert_eq!(matches, ["aXX.md", "a_%.md.more.md"]);
}
