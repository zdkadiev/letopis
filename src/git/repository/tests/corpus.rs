use super::GitRepository;
use crate::process_test_support::{ObjectFormat, TempRepo};
use crate::{domain::RepoPath, git::corpus::parse_candidates, service::DocumentErrorKind};
use std::fs;

#[test]
fn candidates_follow_git_ignore_and_preserve_deleted_tracked_paths() {
    use crate::service::CorpusLister;
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    for path in [
        "deleted.md",
        "tracked-ignored.md",
        "space name.markdown",
        "ignored.md",
        "plain.txt",
    ] {
        fs::write(repo.get_root().join(path), b"bytes").unwrap();
    }
    repo.run_git_successfully(&["add", "deleted.md", "tracked-ignored.md"], None);
    fs::remove_file(repo.get_root().join("deleted.md")).unwrap();
    fs::write(repo.get_root().join(".gitignore"), b"*ignored.md\n").unwrap();
    fs::create_dir(repo.get_root().join(".letopis")).unwrap();
    fs::write(repo.get_root().join(".letopis/private.md"), b"private").unwrap();
    let before = repo.capture_fixture_state();
    let objects = repo.run_git_successfully(&["count-objects", "-v"], None);
    let refs = repo.run_git_successfully(&["show-ref"], None);
    let git = GitRepository::open(repo.get_root()).unwrap();
    // When
    let candidates = git.list_candidates().unwrap();
    // Then
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        [
            "deleted.md",
            "live.md",
            "space name.markdown",
            "tracked-ignored.md"
        ]
    );
    assert_eq!(repo.capture_fixture_state(), before);
    assert_eq!(
        repo.run_git_successfully(&["count-objects", "-v"], None),
        objects
    );
    assert_eq!(repo.run_git_successfully(&["show-ref"], None), refs);
}

#[test]
fn shared_repository_observes_index_and_ignore_edits_between_listings() {
    use crate::service::CorpusLister;
    for format in crate::process_test_support::FORMATS {
        // Given: one shared repository whose index and ignore stack have already been read.
        let repo = TempRepo::new(format);
        fs::write(repo.get_root().join("ignored.md"), b"ignored").unwrap();
        fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
        let git = GitRepository::open(repo.get_root()).unwrap();
        let shared = git.clone();
        assert_eq!(
            git.list_candidates().unwrap(),
            [RepoPath::parse("live.md").unwrap()]
        );
        // When: Git stages an ignored file and removes the previous index entry.
        repo.run_git_successfully(&["add", "-f", "ignored.md"], None);
        repo.run_git_successfully(&["rm", "--cached", "-f", "live.md"], None);
        fs::write(repo.get_root().join(".gitignore"), b"ignored.md\nlive.md\n").unwrap();
        // Then: the clone sees the new index and fresh ignore bytes without reopening.
        assert_eq!(
            shared.list_candidates().unwrap(),
            [RepoPath::parse("ignored.md").unwrap()]
        );
        // When: remove the tracked file and make the other live file eligible again.
        repo.run_git_successfully(&["rm", "--cached", "-f", "ignored.md"], None);
        fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
        // Then: the original handle also refreshes its index/exclude observation.
        assert_eq!(
            git.list_candidates().unwrap(),
            [RepoPath::parse("live.md").unwrap()]
        );
    }
}

#[test]
fn candidates_reject_unsupported_raw_paths() {
    use crate::service::{CorpusLister, DocumentErrorKind};
    for raw in [
        b"bad\xff.md".to_vec(),
        b"bad#anchor.md".to_vec(),
        b"bad\nline.md".to_vec(),
    ] {
        // Given
        let repo = TempRepo::new(ObjectFormat::Sha1);
        // Git's index supports raw paths even when the host filesystem rejects them.
        let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend(raw);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        let git = GitRepository::open(repo.get_root()).unwrap();
        // When
        let error = git.list_candidates().unwrap_err();
        // Then
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        assert!(!error.to_string().contains("bad"));
    }
}

#[test]
fn candidates_deduplicate_unmerged_index_stages() {
    use crate::service::CorpusLister;
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"conflict"));
    let entries = format!(
        "0 {}\tconflict.md\n100644 {oid} 1\tconflict.md\n100644 {oid} 2\tconflict.md\n100644 {oid} 3\tconflict.md\n",
        "0".repeat(40)
    );
    repo.run_git_successfully(&["update-index", "--index-info"], Some(entries.as_bytes()));
    let git = GitRepository::open(repo.get_root()).unwrap();
    // When
    let candidates = git.list_candidates().unwrap();
    // Then
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        ["conflict.md", "live.md"]
    );
}

#[test]
fn candidates_exclude_metadata_before_validating_paths() {
    use crate::service::CorpusLister;
    for raw in [
        b".letopis/bad#anchor.md".as_slice(),
        b".letopis/bad\xff.md",
        b"nested/.letopis/bad\xff.markdown",
    ] {
        // Given: Git can list metadata names unsupported by the corpus model.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join(".letopis-notes.md"), b"candidate").unwrap();
        let oid = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend(raw);
        entry.push(0);
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        let git = GitRepository::open(repo.get_root()).unwrap();
        // When
        let candidates = git.list_candidates().unwrap();
        // Then
        assert_eq!(
            candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
            [".letopis-notes.md", "live.md"]
        );
    }
}

#[test]
fn candidates_exclude_raw_git_metadata_components() {
    // Given: Git itself forbids adding .git paths to its real index.
    let raw = b".git/bad#anchor.md\0nested/.git/bad\xff.markdown\0.git-notes.md\0";
    // When
    let candidates = parse_candidates(raw).unwrap();
    // Then
    assert_eq!(
        candidates.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        [".git-notes.md"]
    );
}

#[test]
fn rejects_unterminated_candidates_and_accepts_empty_output() {
    // Given / When
    let malformed = parse_candidates(b"note.md").unwrap_err();
    let empty = parse_candidates(b"").unwrap();
    // Then
    assert_eq!(malformed.get_kind(), DocumentErrorKind::InternalError);
    assert!(empty.is_empty());
}

#[test]
fn corpus_matches_git_for_nested_excludes_staged_deletion_and_symlinks() {
    use crate::service::CorpusLister;
    // Given: index membership and three exclude sources disagree with live files.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    prepare_exclude_fixture(&repo);
    let before = repo.capture_fixture_state();
    let expected = [
        "dir.md/inside.md",
        "live.md",
        "module.md",
        "nested/keep.md",
        "staged-delete.md",
        "symlink.md",
        "tracked-ignore.md",
    ];
    let oracle = repo.run_git_successfully(
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        None,
    );
    let mut oracle: Vec<_> = oracle
        .split(|byte| *byte == 0)
        .filter(|path| path.ends_with(b".md"))
        .map(|path| std::str::from_utf8(path).unwrap())
        .collect();
    oracle.sort_unstable();
    assert_eq!(oracle, expected);
    // When: the library lists the current worktree's real index and ignore stack.
    let actual = GitRepository::open(repo.get_root())
        .unwrap()
        .list_candidates()
        .unwrap();
    // Then: each independent oracle path is present once and user state stays unchanged.
    assert_eq!(
        actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(repo.capture_fixture_state(), before);
}

#[test]
fn linked_worktree_listing_uses_its_own_index() {
    use crate::service::CorpusLister;
    // Given: two worktrees share objects but have distinct staged paths.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    repo.run_git_successfully(
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
        None,
    );
    fs::write(linked.join("linked-only.md"), b"linked").unwrap();
    repo.run_git_successfully(
        &["-C", linked.to_str().unwrap(), "add", "linked-only.md"],
        None,
    );
    fs::remove_file(linked.join("linked-only.md")).unwrap();
    let expected = repo.run_git_successfully(
        &[
            "-C",
            linked.to_str().unwrap(),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        None,
    );
    // When: discovery begins in the linked worktree rather than the common gitdir.
    let actual = GitRepository::open(&linked)
        .unwrap()
        .list_candidates()
        .unwrap();
    // Then: the deleted-but-staged linked path is retained and the other index is untouched.
    assert!(
        expected
            .split(|byte| *byte == 0)
            .any(|path| path == b"linked-only.md")
    );
    assert!(actual.contains(&RepoPath::parse("linked-only.md").unwrap()));
    assert!(
        !GitRepository::open(repo.get_root())
            .unwrap()
            .list_candidates()
            .unwrap()
            .contains(&RepoPath::parse("linked-only.md").unwrap())
    );
}

#[test]
fn corpus_ignores_replacements_for_index_backed_ignore_files() {
    use crate::service::CorpusLister;
    // Given: a sparse ignore file has a replacement that would expose a hidden document.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    fs::write(repo.get_root().join(".gitignore"), b"hidden.md\n").unwrap();
    fs::write(repo.get_root().join("hidden.md"), b"hidden").unwrap();
    repo.run_git_successfully(&["add", ".gitignore"], None);
    let original = repo.read_oid(&["rev-parse", ":.gitignore"], None);
    let replacement = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b""));
    repo.run_git_successfully(&["replace", &original, &replacement], None);
    repo.run_git_successfully(&["update-index", "--skip-worktree", ".gitignore"], None);
    fs::remove_file(repo.get_root().join(".gitignore")).unwrap();
    // When: candidate enumeration falls back to the index's raw ignore blob.
    let actual = GitRepository::open(repo.get_root())
        .unwrap()
        .list_candidates()
        .unwrap();
    // Then: replacement refs cannot change corpus membership.
    assert_eq!(
        actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
        ["live.md"]
    );
}

fn prepare_exclude_fixture(repo: &TempRepo) {
    let commit = repo.read_oid(&["rev-parse", "HEAD"], None);
    repo.run_git_successfully(
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},module.md"),
        ],
        None,
    );
    fs::create_dir(repo.get_root().join("module.md")).unwrap();
    fs::write(repo.get_root().join("module.md/hidden.md"), b"submodule").unwrap();
    for path in ["tracked-ignore.md", "staged-delete.md"] {
        fs::write(repo.get_root().join(path), b"tracked").unwrap();
    }
    repo.run_git_successfully(&["add", "tracked-ignore.md", "staged-delete.md"], None);
    repo.run_git_successfully(&["rm", "--cached", "staged-delete.md"], None);
    for directory in ["nested", "dir.md"] {
        fs::create_dir(repo.get_root().join(directory)).unwrap();
    }
    for path in [
        "nested/keep.md",
        "nested/drop.md",
        "dir.md/inside.md",
        "info-hidden.md",
        "global-hidden.md",
    ] {
        fs::write(repo.get_root().join(path), b"bytes").unwrap();
    }
    fs::write(repo.get_root().join(".gitignore"), b"tracked-ignore.md\n").unwrap();
    fs::write(
        repo.get_root().join("nested/.gitignore"),
        b"*.md\n!keep.md\n",
    )
    .unwrap();
    fs::write(
        repo.get_root().join(".git/info/exclude"),
        b"info-hidden.md\n",
    )
    .unwrap();
    let global = repo.get_root().join("global-excludes");
    fs::write(&global, b"global-hidden.md\n").unwrap();
    repo.run_git_successfully(
        &["config", "core.excludesFile", global.to_str().unwrap()],
        None,
    );
    std::os::unix::fs::symlink("missing-target", repo.get_root().join("symlink.md")).unwrap();
}
