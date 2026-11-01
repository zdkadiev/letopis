mod support;

use letopis::{
    domain::{LineRange, Sha256},
    service::{NodeMetadata, Refresher, Storage, StorageRead},
    wire_document_refresher,
};
use serde_json::{Value, json};
use support::{
    documents::{DocumentRepo, build_external, build_whole},
    reading::ReadingRepo,
};

#[test]
fn publish_opaque_remote_without_local_interpretation() {
    // Given
    let repo = DocumentRepo::new();
    let address = "https://Host/a?x=1#section";
    repo.write("remote.md", build_external(7, address, "RemoteToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();

    refresher.refresh().unwrap();

    let state = repo.get_state();
    let NodeMetadata::Source(source) = &state.get_node(address).metadata else {
        panic!("expected remote source")
    };
    // When / Then
    assert_eq!(source.uri.as_str(), address);
    assert_eq!(
        (source.hash, source.lines, source.created_at),
        (None, None, 7)
    );
    assert_eq!(repo.search("RemoteToken"), ["remote.md"]);
}

#[test]
fn merge_equivalent_local_ranges_with_minimum_reference_time() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write(
        "early.md",
        build_external(3, "./source.txt#L01-L01", "EarlyToken"),
    );
    repo.write(
        "late.md",
        build_external(8, "source.txt#L1-L1", "LateToken"),
    );
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();

    refresher.refresh().unwrap();

    let state = repo.get_state();
    // When / Then
    assert_eq!(state.nodes.len(), 3);
    let NodeMetadata::Source(source) = &state.get_node("source.txt#L1-L1").metadata else {
        panic!("expected local source")
    };
    assert_eq!(source.created_at, 3);
    assert_eq!(source.lines, Some(LineRange::new(1, 1).unwrap()));
    assert_eq!(
        source.hash,
        Some(
            Sha256::parse("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
                .unwrap()
        )
    );
}

#[test]
fn reject_ambiguous_sources_before_fresh_cache_publication_and_allow_repair() {
    // Given: each invalid address gets its own fresh Git/SQLite fixture.
    for address in [
        "./https:notes",
        "dir/../HTTP:notes",
        "./https://Host/a",
        "./hTtPs:notes#L01-L02",
    ] {
        let repo = ReadingRepo::new();
        repo.write("https:notes", b"ordinary local source");
        repo.write("HTTP:notes", b"ordinary local source");
        repo.write("https:/Host/a", b"ordinary local source");
        repo.write(
            "bad.md",
            build_external(3, address, "SECRET_RAW_BODY").as_bytes(),
        );

        // When / Then: check refreshes the real cache and returns safe file diagnostics.
        for _ in 0..2 {
            // When / Then: the invalid file contributes no partial nodes, edges or searchable text.
            assert_eq!(
                run_check(&repo),
                json!({"ok":true,"result":{"errors":[{
                    "code":"validation_failed", "message":"invalid graph target",
                    "details":{"path":"bad.md","reason":"invalid_target"}
                }]}})
            );
            let mut storage = repo.open_storage();
            let mut view = storage.begin_read_tx().unwrap();
            assert!(view.get_nodes().unwrap().is_empty());
            assert!(view.get_edges_by_source_id(None).unwrap().is_empty());
            let files = view.get_files().unwrap();
            assert_eq!(files.len(), 2); // ReadingRepo also contains unmarked live.md.
            let bad = files
                .iter()
                .find(|file| file.path.as_str() == "bad.md")
                .unwrap();
            assert!(!bad.valid);
        }

        // Given: editing away the bad link allows recovery without a cache rebuild.
        repo.write("bad.md", build_whole(3, "RepairedToken").as_bytes());
        // When / Then: the repaired file publishes without rebuilding the cache.
        assert_eq!(run_check(&repo), json!({"ok":true,"result":{"errors":[]}}));
        let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
        assert!(!refresher.refresh().unwrap().published);
        let mut storage = repo.open_storage();
        let mut view = storage.begin_read_tx().unwrap();
        assert_eq!(view.get_nodes().unwrap().len(), 1);
        assert!(view.get_edges_by_source_id(None).unwrap().is_empty());
        assert_eq!(
            view.get_text_matches_by_expression("RepairedToken")
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn retain_last_valid_projection_after_ambiguous_source_edit() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("valid.md", build_external(3, "source.txt", "RetainedToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    let before = repo.get_state();

    // When
    repo.write(
        "valid.md",
        build_external(3, "./https:notes", "SECRET_RAW_BODY"),
    );
    let result = refresher.refresh().unwrap();

    // Then: invalid-file policy preserves the readable effective graph and FTS.
    assert_eq!(result.invalid_files_updated, 1);
    let invalid = repo.get_state();
    assert!(!invalid.get_file("valid.md").valid);
    assert_eq!(invalid.nodes, before.nodes);
    assert_eq!(invalid.edges, before.edges);
    assert_eq!(repo.search("RetainedToken"), ["valid.md"]);
    assert!(repo.search("SECRET_RAW_BODY").is_empty());

    // Given: the invalid referrer is deleted.
    repo.remove("valid.md");
    // When
    refresher.refresh().unwrap();
    let removed = repo.get_state();
    // Then
    assert!(removed.nodes.is_empty());
    assert!(removed.edges.is_empty());
}

fn run_check(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["check"], b"{}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("SECRET_RAW_BODY"));
    assert!(!text.contains("https:notes"));
    serde_json::from_str(&text).unwrap()
}
