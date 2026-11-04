mod support;
use letopis::{
    domain::{ElementKind, LineRange, Sha256},
    service::{DocumentErrorKind, NodeMetadata, Refresher},
    wire_document_refresher,
};
use std::{fs, os::unix::fs::symlink, sync::Barrier, thread};
use support::documents::{DocumentRepo, build_external, build_whole, get_target};

#[test]
fn refresh_publishes_regular_file_and_skips_symlink() {
    // Given: a Markdown file and a corpus candidate symlink to it.
    let repo = DocumentRepo::new();
    repo.write("AGENTS.md", "# a\n");
    symlink("AGENTS.md", repo.get_root().join("CLAUDE.md")).unwrap();
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    // When: the corpus is refreshed.
    let outcome = documents.refresh(&[]).unwrap();
    // Then: the regular file is published and the symlink is absent from files.
    assert!(outcome.published);
    assert_eq!(outcome.files_updated, 1);
    let state = repo.get_state();
    assert_eq!(state.files.len(), 1);
    assert_eq!(state.files[0].path.as_str(), "AGENTS.md");
}

#[test]
fn publish_initial_empty_cache_once() {
    // Given: a fresh repository has no service directory.
    let repo = DocumentRepo::new();
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    // When.
    let first = documents.refresh(&[]).unwrap();
    let second = documents.refresh(&[]).unwrap();
    // Then.
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
    assert!(repo.get_state().nodes.is_empty());
}

#[test]
fn replace_sections_and_preserve_stable_ids_and_corpus_bytes() {
    // Given
    let repo = DocumentRepo::new();
    let whole = build_whole(7, "OriginalToken");
    let sections = "# One {#one}\n<!-- letopis: {\"created_at\":8} -->\nFirstToken\n# Two {#two}\n<!-- letopis: {\"created_at\":9} -->\nSecondToken\n";
    repo.write("whole.md", &whole);
    repo.write("under_% score.markdown", sections);
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    // When
    let first = documents.refresh(&[]).unwrap();
    let original = repo.get_state();
    // Then
    assert_eq!(first.files_updated, 2);
    assert_eq!(original.nodes.len(), 3);
    assert_eq!(
        original.get_file("whole.md").hash,
        Sha256::digest(whole.as_bytes())
    );
    // Given: stable identity, different text, and a removed section.
    let edited = build_whole(7, "ReplacementToken");
    repo.write("whole.md", &edited);
    repo.write(
        "under_% score.markdown",
        "# One {#one}\n<!-- letopis: {\"created_at\":8} -->\nChangedSection\n",
    );
    // When
    documents.refresh(&[]).unwrap();
    let current = repo.get_state();
    // Then
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
    // Given
    let repo = DocumentRepo::new();
    repo.write("old.md", build_whole(1, "OldToken"));
    repo.run_git(&["add", "old.md"]);
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    fs::rename(
        repo.get_root().join("old.md"),
        repo.get_root().join("new.md"),
    )
    .unwrap();
    let renamed = documents.refresh(&[]).unwrap();
    // When / Then
    assert_eq!((renamed.files_updated, renamed.files_removed), (1, 1));
    assert_eq!(repo.search("OldToken"), ["new.md"]);
    assert!(!documents.refresh(&[]).unwrap().published);
    repo.write("new.md", "UnmarkedToken\n");
    documents.refresh(&[]).unwrap();
    assert!(repo.get_state().nodes.is_empty());
    assert!(repo.get_state().get_file("new.md").valid);
    repo.write("new.md", build_whole(1, "ReintroducedToken"));
    documents.refresh(&[]).unwrap();
    assert_eq!(repo.search("ReintroducedToken"), ["new.md"]);
    repo.remove("new.md");
    assert_eq!(documents.refresh(&[]).unwrap().files_removed, 1);
    assert!(repo.get_state().files.is_empty());
    assert!(!documents.refresh(&[]).unwrap().published);
}

#[test]
fn retain_invalid_projection_while_publishing_valid_peer_and_repair() {
    // Given
    let repo = DocumentRepo::new();
    repo.write(
        "a.md",
        build_whole(
            1,
            "RetainedToken [b](b.md \"letopis:type=includes;path=b.md\")",
        ),
    );
    repo.write("b.md", build_whole(2, "BeforeToken"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.write("a.md", "<!-- letopis: SECRET_RAW_INVALID -->\nBadToken");
    repo.write("b.md", build_whole(2, "AfterToken"));
    repo.write("new-invalid.md", [0xff]);
    // When
    let result = documents.refresh(&[]).unwrap();
    let invalid = repo.get_state();
    // Then
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
    assert!(!documents.refresh(&[]).unwrap().published);
    assert_eq!(repo.get_state(), invalid);
    repo.write("a.md", build_whole(1, "RepairedToken"));
    repo.remove("new-invalid.md");
    let repaired = documents.refresh(&[]).unwrap();
    assert_eq!(repaired.files_removed, 1);
    assert!(repo.get_state().get_file("a.md").valid);
    assert!(repo.get_state().edges.is_empty());
    assert!(repo.search("RetainedToken").is_empty());
    assert_eq!(repo.search("RepairedToken"), ["a.md"]);
}

#[test]
fn keep_cycles_dangling_declarations_and_incoming_edges_on_target_deletion() {
    // Given
    let repo = DocumentRepo::new();
    repo.write(
        "a.md",
        build_whole(
            1,
            "[b](b.md \"letopis:type=includes;path=b.md\") [missing](missing.md \"letopis:type=supports;path=missing.md\")",
        ),
    );
    repo.write(
        "b.md",
        build_whole(2, "[a](a.md \"letopis:type=includes;path=a.md\")"),
    );
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let state = repo.get_state();
    // When / Then
    assert_eq!(state.edges.len(), 3);
    let NodeMetadata::Knowledge(a) = &state.get_node("a.md").metadata else {
        panic!()
    };
    assert_eq!(a.kind, ElementKind::Metavertex);
    repo.remove("b.md");
    documents.refresh(&[]).unwrap();
    let state = repo.get_state();
    assert_eq!(state.edges.len(), 2);
    assert!(
        state
            .edges
            .iter()
            .any(|e| e.get_target() == &get_target("b.md", false))
    );
    repo.write("b.md", build_whole(2, "TargetReturns"));
    documents.refresh(&[]).unwrap();
    assert_eq!(repo.get_state().edges, state.edges);
}

#[test]
fn reconcile_shared_source_minimum_and_retain_invalid_references() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\r\ntwo\r\nthree\r\nfour");
    repo.write(
        "a.md",
        build_external(3, "source.txt#L02-L03", "EarlyToken"),
    );
    repo.write("b.md", build_external(8, "./source.txt#L2-L3", "LateToken"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    // When
    let before = repo.get_state();
    let NodeMetadata::Source(s) = &before.get_node("source.txt#L2-L3").metadata else {
        panic!()
    };
    // Then
    assert_eq!(s.created_at, 3);
    assert_eq!(s.hash, Some(Sha256::digest(b"two\r\nthree\r\n")));
    assert_eq!(s.lines, Some(LineRange::new(2, 3).unwrap()));
    repo.write("b.md", "<!-- letopis: invalid -->");
    repo.remove("a.md");
    documents.refresh(&[]).unwrap();
    let state = repo.get_state();
    let NodeMetadata::Source(s) = &state.get_node("source.txt#L2-L3").metadata else {
        panic!()
    };
    assert_eq!(s.created_at, 8);
    assert_eq!(state.edges.len(), 1);
    repo.remove("b.md");
    documents.refresh(&[]).unwrap();
    assert!(repo.get_state().nodes.is_empty());
    assert!(repo.get_state().edges.is_empty());
}

#[test]
fn hash_exact_whole_source_and_represent_missing_or_out_of_range_sources() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"a\r\nb\nc");
    repo.write("a.md", build_external(3, "source.txt", "Whole"));
    repo.write("b.md", build_external(4, "source.txt#L2-L9", "Range"));
    repo.write("c.md", build_external(5, "absent.txt", "Missing"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let state = repo.get_state();
    for (id, hash) in [
        ("source.txt", Some(Sha256::digest(b"a\r\nb\nc"))),
        ("source.txt#L2-L9", None),
        ("absent.txt", None),
    ] {
        let NodeMetadata::Source(s) = &state.get_node(id).metadata else {
            panic!()
        };
        // When / Then
        assert_eq!(s.hash, hash);
    }
}

#[test]
fn reject_source_over_existing_knowledge_atomically() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("a.md", build_whole(1, "OriginalKnowledge"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    // When
    let before = repo.get_state();
    repo.write("b.md", build_external(2, "a.md", "NewSourceReference"));
    // Then
    assert_eq!(
        documents.refresh(&[]).unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
    assert_eq!(repo.get_state(), before);
    assert_eq!(repo.search("OriginalKnowledge"), ["a.md"]);
    assert!(repo.search("NewSourceReference").is_empty());
    // The inverse role becomes unambiguous after removing the knowledge projection.
    repo.write("a.md", "source bytes\n");
    documents.refresh(&[]).unwrap();
    assert!(matches!(
        repo.get_state().get_node("a.md").metadata,
        NodeMetadata::Source(_)
    ));
    assert!(repo.search("OriginalKnowledge").is_empty());
}

#[test]
fn reject_knowledge_over_referenced_source_and_allow_orphan_role_transition() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("a.md", "source bytes\n");
    repo.write("z.md", build_external(2, "a.md", "ReferenceToken"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    // When
    let before = repo.get_state();
    repo.write("a.md", build_whole(1, "NewKnowledge"));
    // Then
    assert_eq!(
        documents.refresh(&[]).unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
    assert_eq!(repo.get_state(), before);
    assert!(repo.search("NewKnowledge").is_empty());
    repo.write("z.md", build_whole(2, "NoReference"));
    documents.refresh(&[]).unwrap();
    let state = repo.get_state();
    assert!(matches!(
        state.get_node("a.md").metadata,
        NodeMetadata::Knowledge(_)
    ));
    assert_eq!(repo.search("NewKnowledge"), ["a.md"]);
    assert!(state.edges.is_empty());
}

#[test]
fn unsafe_source_read_rolls_back_all_cache_changes() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("a.md", build_whole(1, "PreviousToken"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let before = repo.get_state();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.txt"), "SECRET_OUTSIDE").unwrap();
    symlink(outside.path(), repo.get_root().join("unsafe")).unwrap();
    repo.write(
        "a.md",
        build_external(1, "unsafe/private.txt", "ChangedToken"),
    );
    // When
    let error = documents.refresh(&[]).unwrap_err();
    // Then
    assert!(!format!("{error:?} {error}").contains("SECRET_OUTSIDE"));
    assert_eq!(repo.get_state(), before);
    assert_eq!(repo.search("PreviousToken"), ["a.md"]);
    assert!(repo.search("ChangedToken").is_empty());
}

#[test]
fn recalculate_source_minimum_after_referrer_edit_and_preserve_unaffected_source() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", "OriginalSource");
    repo.write("other.txt", "OtherSource");
    repo.write("a.md", build_external(3, "source.txt", "Early"));
    repo.write("b.md", build_external(8, "source.txt", "Late"));
    repo.write("c.md", build_external(1, "other.txt", "Unchanged"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.write("a.md", build_external(10, "source.txt", "Revised"));
    repo.write("other.txt", "UnrelatedNewSourceBytes");
    documents.refresh(&[]).unwrap();
    // When
    let after = repo.get_state();
    let NodeMetadata::Source(source) = &after.get_node("source.txt").metadata else {
        panic!()
    };
    // Then
    assert_eq!(source.created_at, 8);
    assert_eq!(after.get_node("other.txt"), before.get_node("other.txt"));
    assert!(!documents.refresh(&[]).unwrap().published);
}

#[test]
fn remove_exact_path_without_treating_underscores_or_percent_as_patterns() {
    // Given
    let repo = DocumentRepo::new();
    for path in ["a_%.md", "aXX.md", "a_%.md.more.md"] {
        repo.write(path, build_whole(1, "SharedToken"));
    }
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.remove("a_%.md");
    let outcome = documents.refresh(&[]).unwrap();
    // When
    let after = repo.get_state();
    // Then
    assert_eq!(outcome.files_removed, 1);
    assert_eq!(after.nodes.len(), 2);
    for path in ["aXX.md", "a_%.md.more.md"] {
        assert_eq!(after.get_node(path), before.get_node(path));
    }
    let mut matches = repo.search("SharedToken");
    matches.sort();
    assert_eq!(matches, ["aXX.md", "a_%.md.more.md"]);
}

#[test]
fn reject_cache_database_symlink_without_touching_external_file() {
    // Given: the cache database path
    // When / Then
    assert_database_symlink_rejected("cache.db");
}

#[test]
fn reject_vector_database_symlink_without_touching_external_file() {
    // Given: the vector database path
    // When / Then
    assert_database_symlink_rejected("vectors.db");
}

#[test]
fn simultaneous_document_openers_initialize_and_publish_once() {
    // Given a virgin repository and coordinate both normal composition calls.
    let repo = DocumentRepo::new();
    let barrier = Barrier::new(2);
    // When: both callers open independently and refresh the same empty corpus.
    let outcomes = thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    wire_document_refresher(repo.get_root())
                        .unwrap()
                        .refresh(&[])
                        .unwrap()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    // Then: initialization succeeds for both; publication happens exactly once.
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.published).count(),
        1
    );
    assert!(outcomes.iter().all(|outcome| outcome.revision == 1));
    assert_eq!(repo.get_state().revision, 1);
}

#[test]
fn removes_an_orphan_source_while_preserving_unrelated_knowledge() {
    // Given: one independent knowledge element survives the last source reference.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "source bytes");
    repo.write("reference.md", build_external(2, "source.txt", "Reference"));
    repo.write("unrelated.md", build_whole(3, "SurvivingKnowledge"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh(&[]).unwrap();
    let before = repo.get_state();
    // When
    repo.remove("reference.md");
    documents.refresh(&[]).unwrap();
    // Then: an unrelated ID must not prevent orphan cleanup.
    let after = repo.get_state();
    assert_eq!(after.nodes.len(), 1);
    assert_eq!(
        after.get_node("unrelated.md"),
        before.get_node("unrelated.md")
    );
    assert!(after.edges.is_empty());
    assert_eq!(repo.search("SurvivingKnowledge"), ["unrelated.md"]);
}

fn assert_database_symlink_rejected(name: &str) {
    for initialize in [false, true] {
        // Given an empty or already compatible outside database.
        let repo = DocumentRepo::new();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join(name);
        if initialize {
            drop(
                letopis::open_storage(
                    &outside.path().join("cache.db"),
                    &outside.path().join("vectors.db"),
                )
                .unwrap(),
            );
        } else {
            fs::write(&target, []).unwrap();
        }
        let before = fs::read(&target).unwrap();
        fs::create_dir(repo.get_root().join(".letopis")).unwrap();
        symlink(&target, repo.get_root().join(".letopis").join(name)).unwrap();
        // When.
        let result = wire_document_refresher(repo.get_root());
        // Then rejection and exact outside bytes, including initialized databases.
        let Err(error) = result else {
            panic!("accepted database symlink: {name}");
        };
        assert!(!format!("{error} {error:?}").contains(target.to_str().unwrap()));
        assert_eq!(fs::read(&target).unwrap(), before);
        assert!(!outside.path().join(format!("{name}-wal")).exists());
        assert!(!outside.path().join(format!("{name}-shm")).exists());
    }
}
