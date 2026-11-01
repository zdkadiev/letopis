mod support;

use letopis::{
    domain::{RepoPath, TreeOid},
    open_git_snapshots, open_storage,
    service::{GitSnapshots, NodeMetadata, RefreshOutcome, Refresher, Storage},
    wire_document_refresher,
};
use std::fs;
use support::{
    documents::State,
    git::{FORMATS, FixtureState, TempRepo},
    provenance::{build_note, create_basis},
};

#[test]
fn retain_basis_before_publishing_and_keep_history_after_gc() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        repo.write("source.txt", b"new source\n");
        let old_tree = create_basis(repo.get_root(), &[("source.txt", b"old source\r\n")]);
        repo.write(
            "note.md",
            build_note(Some(&old_tree), &["source.txt"], "Knowledge"),
        );
        let expected_bytes = fs::read(repo.get_root().join("note.md")).unwrap();
        let user_state_before = capture_head_and_index(&repo);
        // When
        refresh(&repo);
        let expected_metadata = repo.get_state().get_node("note.md").metadata.clone();
        // Then
        assert!(has_exact_service_ref(&repo, &old_tree));
        repo.expire_reflogs_gc_prune_now_and_pack_refs();
        assert_eq!(get_retained_bytes(&repo, &old_tree), b"old source\r\n");
        assert_eq!(
            repo.get_state().get_node("note.md").metadata,
            expected_metadata
        );
        assert_eq!(
            fs::read(repo.get_root().join("note.md")).unwrap(),
            expected_bytes
        );
        assert_eq!(capture_head_and_index(&repo), user_state_before);
    }
}

#[test]
fn retain_newly_available_basis_without_markdown_change() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let tree = create_basis(repo.get_root(), &[("source.txt", b"later\n")]);
        let object_path = repo.build_object_path(tree.as_str());
        let object = fs::read(&object_path).unwrap();
        fs::remove_file(&object_path).unwrap();
        repo.write("source.txt", b"later\n");
        repo.write(
            "note.md",
            build_note(Some(&tree), &["source.txt"], "Knowledge"),
        );
        // When
        let first = refresh(&repo);
        // Then
        assert!(!has_exact_service_ref(&repo, &tree));
        assert_eq!(get_basis(&repo.get_state()), Some(tree.clone()));
        // Given
        let before = repo.get_state();
        fs::write(object_path, object).unwrap();
        // When
        let second = refresh(&repo);
        // Then
        assert!(!second.published);
        assert_eq!(second.revision, first.revision);
        assert_eq!(repo.get_state(), before);
        assert!(has_exact_service_ref(&repo, &tree));
    }
}

#[test]
fn reject_corrupt_closure_and_ref_failure_before_publication() {
    // Given
    for corrupt in [true, false] {
        let repo = TempRepo::new(FORMATS[0]);
        repo.write("note.md", build_note(None, &[], "Original"));
        refresh(&repo);
        let before = repo.get_state();
        let tree = create_basis(repo.get_root(), &[("source.txt", b"basis\n")]);
        repo.write("source.txt", b"basis\n");
        repo.write(
            "note.md",
            build_note(Some(&tree), &["source.txt"], "Rejected"),
        );
        if corrupt {
            let blob = repo.get_oid(
                &["rev-parse", &format!("{}:source.txt", tree.as_str())],
                None,
            );
            fs::remove_file(repo.build_object_path(&blob)).unwrap();
            fs::write(repo.build_object_path(&blob), b"invalid object").unwrap();
        } else {
            let refs = repo.get_root().join(".git/refs/letopis");
            fs::create_dir_all(&refs).unwrap();
            fs::write(refs.join("snapshots"), b"block namespace").unwrap();
        }
        let user_state_before = capture_head_and_index(&repo);
        // When
        let refresh_result = wire_document_refresher(repo.get_root()).unwrap().refresh();
        // Then
        assert!(refresh_result.is_err());
        assert_eq!(repo.get_state(), before);
        assert_eq!(capture_head_and_index(&repo), user_state_before);
    }
}

#[test]
fn preserve_declared_basis_on_edit_and_reproduce_it_after_cache_rebuild() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let tree = create_basis(repo.get_root(), &[("source.txt", b"original\n")]);
    repo.write("source.txt", b"original\n");
    repo.write(
        "note.md",
        build_note(Some(&tree), &["source.txt"], "Original"),
    );
    refresh(&repo);
    repo.write("source.txt", b"changed\n");
    let note = build_note(Some(&tree), &["source.txt"], "Edited knowledge");
    repo.write("note.md", &note);
    // When
    refresh(&repo);
    // Then
    assert_eq!(get_basis(&repo.get_state()), Some(tree.clone()));
    remove_cache(&repo);
    refresh(&repo);
    assert_eq!(get_basis(&repo.get_state()), Some(tree.clone()));
    assert!(has_exact_service_ref(&repo, &tree));
    assert_eq!(
        fs::read(repo.get_root().join("note.md")).unwrap(),
        note.as_bytes()
    );
}

#[test]
fn remove_basis_only_when_metadata_field_is_removed() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let tree = create_basis(repo.get_root(), &[("source.txt", b"source\n")]);
    repo.write("source.txt", b"source\n");
    repo.write(
        "note.md",
        build_note(Some(&tree), &["source.txt"], "Original"),
    );
    refresh(&repo);
    repo.write("note.md", build_note(None, &["source.txt"], "Edited"));
    // When
    refresh(&repo);
    // Then
    assert_eq!(get_basis(&repo.get_state()), None);
    remove_cache(&repo);
    refresh(&repo);
    assert_eq!(get_basis(&repo.get_state()), None);
    assert!(has_exact_service_ref(&repo, &tree));
}

#[test]
fn retain_last_valid_basis_after_invalid_edit() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let tree = create_basis(repo.get_root(), &[("source.txt", b"source\n")]);
    repo.write("source.txt", b"source\n");
    repo.write(
        "note.md",
        build_note(Some(&tree), &["source.txt"], "Original"),
    );
    refresh(&repo);
    repo.run_git_successfully(
        &[
            "update-ref",
            "-d",
            &format!("refs/letopis/snapshots/{}", tree.as_str()),
        ],
        None,
    );
    repo.write("note.md", b"<!-- letopis: invalid -->\n");
    // When
    refresh(&repo);
    // Then
    assert!(!repo.get_state().get_file("note.md").valid);
    assert_eq!(get_basis(&repo.get_state()), Some(tree.clone()));
    assert!(has_exact_service_ref(&repo, &tree));
}

#[test]
fn retain_shared_and_unreferenced_declared_basis_once() {
    // Given
    let repo = TempRepo::new(FORMATS[0]);
    let shared = create_basis(repo.get_root(), &[("source.txt", b"source\n")]);
    let unreferenced = create_basis(repo.get_root(), &[("source.txt", b"unreferenced\n")]);
    repo.write("source.txt", b"source\n");
    repo.write("note.md", build_note(Some(&shared), &["source.txt"], "One"));
    repo.write(
        "other.md",
        build_note(Some(&shared), &["source.txt"], "Two"),
    );
    repo.write("orphan.md", build_note(Some(&unreferenced), &[], "Three"));
    // When
    refresh(&repo);
    // Then
    assert!(has_exact_service_ref(&repo, &shared));
    assert!(has_exact_service_ref(&repo, &unreferenced));
    let refs = repo.run_git_successfully(
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/letopis/snapshots/",
        ],
        None,
    );
    assert_eq!(
        refs.split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
        2
    );
}

fn refresh(repo: &TempRepo) -> RefreshOutcome {
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap()
}
fn has_exact_service_ref(repo: &TempRepo, tree: &TreeOid) -> bool {
    let out = repo.run_git(
        &[
            "show-ref",
            "--verify",
            "--hash",
            &format!("refs/letopis/snapshots/{}", tree.as_str()),
        ],
        None,
    );
    out.status.success() && String::from_utf8(out.stdout).unwrap().trim() == tree.as_str()
}
fn capture_head_and_index(repo: &TempRepo) -> FixtureState {
    repo.capture_fixture_state()
}
fn get_retained_bytes(repo: &TempRepo, tree: &TreeOid) -> Vec<u8> {
    open_git_snapshots(repo.get_root())
        .unwrap()
        .get_blob(tree, &RepoPath::parse("source.txt").unwrap())
        .unwrap()
}
fn get_basis(state: &State) -> Option<TreeOid> {
    let NodeMetadata::Knowledge(node) = &state.get_node("note.md").metadata else {
        panic!("knowledge")
    };
    node.source_snapshot.clone()
}
fn remove_cache(repo: &TempRepo) {
    for name in [
        "cache.db",
        "cache.db-wal",
        "cache.db-shm",
        "cache.db-journal",
    ] {
        let path = repo.get_root().join(".letopis").join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}
trait IndexFixture {
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>);
    fn get_state(&self) -> State;
}
impl IndexFixture for TempRepo {
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.get_root().join(path), bytes).unwrap();
    }
    fn get_state(&self) -> State {
        let mut storage = open_storage(
            &self.get_root().join(".letopis/cache.db"),
            &self.get_root().join(".letopis/vectors.db"),
        )
        .unwrap();
        let mut read = storage.begin_read_tx().unwrap();
        State {
            revision: read.get_revision(),
            nodes: read.get_nodes().unwrap(),
            edges: read.get_edges_by_source_id(None).unwrap(),
            files: read.get_files().unwrap(),
        }
    }
}
