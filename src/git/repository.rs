use super::{cleanup::GitWriteCleanup, corpus, objects, reader::ObjectReader, refs};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{
        CorpusLister, DocumentResult, GitSnapshots, RetainedTree, SnapshotEntry, SnapshotError,
        SnapshotErrorKind, SnapshotFile, SnapshotState, SnapshotVerifier,
    },
};
use std::{
    cell::RefCell,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static REPOSITORY_OPEN_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// One foreground-owned repository and uncached raw reader, shared within a command.
#[derive(Clone, Debug)]
pub(crate) struct GitRepository {
    pub(super) repo: Rc<gix::Repository>,
    pub(super) reader: Rc<RefCell<ObjectReader>>,
    pub(super) root: PathBuf,
    pub(super) oid_len: usize,
    pub(super) cleanup: Arc<GitWriteCleanup>,
    alternates: PathBuf,
}
impl GitRepository {
    /// Discover a self-contained worktree without executable helpers or environment routing.
    pub(crate) fn open(start: &Path) -> Result<Self, SnapshotError> {
        Self::open_with_cleanup(start, Arc::new(GitWriteCleanup::new()))
    }
    /// Share the command's actual write-resource owner with the signal callback.
    pub(crate) fn open_with_cleanup(
        start: &Path,
        cleanup: Arc<GitWriteCleanup>,
    ) -> Result<Self, SnapshotError> {
        let mut permissions = gix::open::Permissions::default();
        permissions.env = gix::open::permissions::Environment::isolated();
        permissions.env.home = gix::sec::Permission::Allow;
        permissions.env.xdg_config_home = gix::sec::Permission::Allow;
        permissions.config.env = false;
        permissions.config.git_binary = false;
        let options = gix::open::Options::default()
            .permissions(permissions)
            .bail_if_untrusted(true)
            .config_overrides(["core.useReplaceRefs=false"]);
        #[cfg(test)]
        REPOSITORY_OPEN_COUNT.with(|count| count.set(count.get() + 1));
        let mut repo = gix::ThreadSafeRepository::discover_opts(
            start,
            Default::default(),
            gix::sec::trust::Mapping {
                full: options.clone(),
                reduced: options,
            },
        )
        .map_err(|error| classify_open_error(&error))?
        .to_thread_local();
        require_worktree_start(&repo, start)?;
        repo.objects.unset_object_cache();
        repo.objects.unset_pack_cache();
        let root = repo.workdir().ok_or_else(|| {
            SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "Git working copy required",
            )
        })?;
        let root = fs::canonicalize(root).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git root")
        })?;
        if root.to_str().is_none() {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "invalid Git root",
            ));
        }
        let oid_len = repo.object_hash().len_in_hex();
        let alternates = repo.common_dir().join("objects/info/alternates");
        require_absent_alternates(&alternates)?;
        let reader = Rc::new(RefCell::new(ObjectReader::new(&repo, oid_len)?));
        Ok(Self {
            repo: Rc::new(repo),
            reader,
            root,
            oid_len,
            cleanup,
            alternates,
        })
    }
    /// Count actual library discovery calls on this test thread, excluding clones.
    #[cfg(test)]
    pub(crate) fn get_repository_open_count() -> usize {
        REPOSITORY_OPEN_COUNT.with(Cell::get)
    }
    /// Return the discovered worktree root.
    pub(crate) fn get_root(&self) -> &Path {
        &self.root
    }
    /// Recheck policy at every operation; an earlier successful open is not authority.
    pub(super) fn require_self_contained_objects(&self) -> Result<(), SnapshotError> {
        require_absent_alternates(&self.alternates)
    }
}
impl GitSnapshots for GitRepository {
    fn retain(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.require_self_contained_objects()?;
        refs::Retainer { git: self }.retain(tree)
    }
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.create_tree(files)
    }
    fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.write_blob(bytes)
    }
    fn create_tree_from_blobs(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.create_tree_from_blobs(entries)
    }
    fn get_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
        self.require_self_contained_objects()?;
        objects::ObjectDb { git: self }.get_blob(tree, path)
    }
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        self.require_self_contained_objects()?;
        refs::Retainer { git: self }.pin(tree)
    }
}

impl SnapshotVerifier for GitRepository {
    fn verify_snapshot(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.require_self_contained_objects()?;
        match (objects::ObjectDb { git: self }).validate_tree(tree) {
            Ok(()) => Ok(SnapshotState::Available),
            Err(error) => match error.get_kind() {
                SnapshotErrorKind::NotFound => Ok(SnapshotState::Missing),
                SnapshotErrorKind::InvalidRequest => Ok(SnapshotState::Invalid),
                SnapshotErrorKind::InternalError => Err(error),
            },
        }
    }
}

impl CorpusLister for GitRepository {
    fn get_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        corpus::get_candidates(self, &self.root)
    }
}
fn require_absent_alternates(path: &Path) -> Result<(), SnapshotError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "Git object alternates are unsupported; use a self-contained clone",
        )),
        Err(_) => Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "cannot inspect Git object alternates",
        )),
    }
}
fn require_worktree_start(repo: &gix::Repository, start: &Path) -> Result<(), SnapshotError> {
    let start = fs::canonicalize(start).map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "invalid Git start directory",
        )
    })?;
    for metadata in [repo.git_dir(), repo.common_dir()] {
        let metadata = fs::canonicalize(metadata).map_err(|_| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "invalid Git metadata directory",
            )
        })?;
        if start.starts_with(metadata) {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "Git working copy required",
            ));
        }
    }
    Ok(())
}
fn classify_open_error(error: &gix::Error) -> SnapshotError {
    if error
        .downcast_any_ref::<gix::discover::upwards::Error>()
        .is_some()
    {
        SnapshotError::new(
            SnapshotErrorKind::InvalidRequest,
            "Git working repository required",
        )
    } else {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            "cannot open Git repository",
        )
    }
}
#[cfg(test)]
mod tests {
    use super::GitRepository;
    use crate::git::constants::SNAPSHOT_REF_PREFIX;
    use crate::process_test_support as fixture;
    use crate::{
        domain::RepoPath,
        open_git_snapshots,
        service::{GitSnapshots, SnapshotErrorKind, SnapshotFile},
    };
    use fixture::{ObjectFormat, TempRepo};
    use std::{fs, path::PathBuf, rc::Rc, sync::Arc};

    mod corpus {
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
            let candidates = git.get_candidates().unwrap();
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
                    git.get_candidates().unwrap(),
                    [RepoPath::parse("live.md").unwrap()]
                );
                // When: Git stages an ignored file and removes the previous index entry.
                repo.run_git_successfully(&["add", "-f", "ignored.md"], None);
                repo.run_git_successfully(&["rm", "--cached", "-f", "live.md"], None);
                fs::write(repo.get_root().join(".gitignore"), b"ignored.md\nlive.md\n").unwrap();
                // Then: the clone sees the new index and fresh ignore bytes without reopening.
                assert_eq!(
                    shared.get_candidates().unwrap(),
                    [RepoPath::parse("ignored.md").unwrap()]
                );
                // When: remove the tracked file and make the other live file eligible again.
                repo.run_git_successfully(&["rm", "--cached", "-f", "ignored.md"], None);
                fs::write(repo.get_root().join(".gitignore"), b"ignored.md\n").unwrap();
                // Then: the original handle also refreshes its index/exclude observation.
                assert_eq!(
                    git.get_candidates().unwrap(),
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
                let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
                let mut entry = format!("100644 {oid}\t").into_bytes();
                entry.extend(raw);
                entry.push(0);
                repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
                let git = GitRepository::open(repo.get_root()).unwrap();
                // When
                let error = git.get_candidates().unwrap_err();
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
            let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"conflict"));
            let entries = format!(
                "0 {}\tconflict.md\n100644 {oid} 1\tconflict.md\n100644 {oid} 2\tconflict.md\n100644 {oid} 3\tconflict.md\n",
                "0".repeat(40)
            );
            repo.run_git_successfully(&["update-index", "--index-info"], Some(entries.as_bytes()));
            let git = GitRepository::open(repo.get_root()).unwrap();
            // When
            let candidates = git.get_candidates().unwrap();
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
                let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
                let mut entry = format!("100644 {oid}\t").into_bytes();
                entry.extend(raw);
                entry.push(0);
                repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
                let git = GitRepository::open(repo.get_root()).unwrap();
                // When
                let candidates = git.get_candidates().unwrap();
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
                .get_candidates()
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
                .get_candidates()
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
                    .get_candidates()
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
            let original = repo.get_oid(&["rev-parse", ":.gitignore"], None);
            let replacement = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b""));
            repo.run_git_successfully(&["replace", &original, &replacement], None);
            repo.run_git_successfully(&["update-index", "--skip-worktree", ".gitignore"], None);
            fs::remove_file(repo.get_root().join(".gitignore")).unwrap();
            // When: candidate enumeration falls back to the index's raw ignore blob.
            let actual = GitRepository::open(repo.get_root())
                .unwrap()
                .get_candidates()
                .unwrap();
            // Then: replacement refs cannot change corpus membership.
            assert_eq!(
                actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
                ["live.md"]
            );
        }

        #[test]
        fn sparse_index_expands_index_tree_membership_without_changing_user_state() {
            use crate::service::CorpusLister;
            for format in crate::process_test_support::FORMATS {
                // Given: a collapsed staged tree differs from HEAD and restored files match ignores.
                let repo = TempRepo::new(format);
                let tree = repo.prepare_sparse_index();
                let expected = [
                    "keep/a.md",
                    "live.md",
                    "omit/b.md",
                    "omit/deleted.md",
                    "omit/nested/c.markdown",
                    "omit/staged.md",
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
                    .filter(|path| path.ends_with(b".md") || path.ends_with(b".markdown"))
                    .map(|path| std::str::from_utf8(path).unwrap())
                    .collect();
                oracle.sort_unstable();
                assert_eq!(oracle, expected);
                let empty = repo.get_oid(&["mktree"], Some(b""));
                repo.run_git_successfully(&["replace", &tree, &empty], None);
                let before = repo.capture_fixture_state();
                // When: listing expands the actual index's tree, including deleted tracked paths.
                let actual = GitRepository::open(repo.get_root())
                    .unwrap()
                    .get_candidates()
                    .unwrap();
                // Then: tracked paths survive ignores, untracked ignored paths do not.
                assert_eq!(
                    actual.iter().map(RepoPath::as_str).collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(repo.capture_fixture_state(), before);
            }
        }

        #[test]
        fn sparse_index_rejects_missing_corrupt_trees_and_late_alternates_safely() {
            use crate::service::CorpusLister;
            for format in crate::process_test_support::FORMATS {
                for failure in ["missing", "corrupt", "alternates"] {
                    // Given: a valid collapsed index was opened before its object store changed.
                    let repo = TempRepo::new(format);
                    let tree = repo.prepare_sparse_index();
                    let git = GitRepository::open(repo.get_root()).unwrap();
                    match failure {
                        "missing" => fs::remove_file(repo.build_object_path(&tree)).unwrap(),
                        "corrupt" => {
                            let empty = repo.get_oid(&["mktree"], Some(b""));
                            repo.replace_loose_object(&tree, &empty);
                        }
                        _ => fs::write(
                            repo.get_root().join(".git/objects/info/alternates"),
                            b"PRIVATE_DONOR_PATH\n",
                        )
                        .unwrap(),
                    }
                    let before = repo.capture_fixture_state();
                    // When: listing needs the collapsed tree rather than trusting HEAD or prior policy.
                    let error = git.get_candidates().unwrap_err();
                    // Then: no incomplete successful listing or raw object/path payload escapes.
                    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
                    assert_eq!(error.to_string(), "cannot list document candidates");
                    assert_eq!(repo.capture_fixture_state(), before);
                }
            }
        }

        fn prepare_exclude_fixture(repo: &TempRepo) {
            let commit = repo.get_oid(&["rev-parse", "HEAD"], None);
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
            std::os::unix::fs::symlink("missing-target", repo.get_root().join("symlink.md"))
                .unwrap();
        }
    }

    #[test]
    fn repository_open_counter_excludes_clones() {
        // Given: an independently prepared repository and this thread's open count.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let before = GitRepository::get_repository_open_count();
        // When: one discovery is followed by a cheap clone and an independent open.
        let git = GitRepository::open(repo.get_root()).unwrap();
        let shared = git.clone();
        // Then: clones share both the repository and reader without discovering again.
        assert_eq!(GitRepository::get_repository_open_count() - before, 1);
        assert!(Rc::ptr_eq(&git.repo, &shared.repo));
        assert!(Rc::ptr_eq(&git.reader, &shared.reader));
        assert!(Arc::ptr_eq(&git.cleanup, &shared.cleanup));
        // When: an independent caller opens the same worktree.
        let independent = GitRepository::open(repo.get_root()).unwrap();
        // Then: it owns fresh state and performs its own discovery.
        assert_eq!(GitRepository::get_repository_open_count() - before, 2);
        assert!(!Rc::ptr_eq(&git.repo, &independent.repo));
    }

    #[test]
    fn assembly_checks_unique_oids_once_without_weakening_supplied_or_final_validation() {
        use crate::service::SnapshotEntry;
        for format in fixture::FORMATS {
            // Given: one independently written blob supplied under three distinct paths.
            let repo = TempRepo::new(format);
            let git = GitRepository::open(repo.get_root()).unwrap();
            let oid = git.write_blob(b"original bytes").unwrap();
            let entries = ["a.md", "b.md", "nested/c.md"].map(|path| SnapshotEntry {
                path: RepoPath::parse(path).unwrap(),
                oid: oid.clone(),
            });
            let before = git.reader.borrow().get_object_read_count();
            // When: assemble from repeated caller-supplied object identities.
            let tree = git.create_tree_from_blobs(&entries).unwrap();
            // Then: exactly one addressed verification occurs in this assembly.
            assert_eq!(git.reader.borrow().get_object_read_count() - before, 1);
            git.pin(&tree).unwrap();
            // Given: valid compressed bytes replace that blob under its old identity.
            let other = git.write_blob(b"corrupt substitute").unwrap();
            repo.replace_loose_object(oid.as_str(), other.as_str());
            // When / Then: neither another assembly nor strong pin trusts the prior success/ref.
            assert_eq!(
                git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
                SnapshotErrorKind::InternalError
            );
            assert_eq!(
                git.pin(&tree).unwrap_err().get_kind(),
                SnapshotErrorKind::InternalError
            );
        }
    }

    #[test]
    fn snapshots_work_without_git_executable() {
        use crate::{
            domain::TreeOid,
            service::{CorpusLister, SnapshotState, SnapshotVerifier},
        };
        // Given: historical packed objects, prepared with an independent Git executable.
        let Ok(root) = std::env::var("LETOPIS_CHILD_FREE_ROOT") else {
            supervise_snapshot_formats();
            return;
        };
        let tree = TreeOid::parse(&std::env::var("LETOPIS_CHILD_FREE_TREE").unwrap()).unwrap();
        // When: every snapshot operation runs with an empty executable search path.
        let git = GitRepository::open(std::path::Path::new(&root)).unwrap();
        let path = RepoPath::parse("exact.md").unwrap();
        let blob = git.write_blob(b"raw\r\n\0bytes").unwrap();
        let captured = git
            .create_tree_from_blobs(&[crate::service::SnapshotEntry {
                path: path.clone(),
                oid: blob,
            }])
            .unwrap();
        let empty = git.create_tree(&[]).unwrap();
        git.retain(&tree).unwrap();
        git.pin(&tree).unwrap();
        git.pin(&captured).unwrap();
        // Then: old and new snapshots remain usable, with exact bytes and corpus membership.
        assert_eq!(
            git.verify_snapshot(&tree).unwrap(),
            SnapshotState::Available
        );
        assert_eq!(
            git.verify_snapshot(&empty).unwrap(),
            SnapshotState::Available
        );
        assert_eq!(git.get_blob(&captured, &path).unwrap(), b"raw\r\n\0bytes");
        assert!(
            git.get_candidates()
                .unwrap()
                .contains(&RepoPath::parse("live.md").unwrap())
        );
    }

    #[test]
    fn strong_pin_rejects_malformed_nested_trees_like_git() {
        // Given: valid roots wrap malformed nested trees written only with --literally.
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let git = GitRepository::open(repo.get_root()).unwrap();
            let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b""));
            let raw_oid: Vec<_> = (0..blob.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&blob[i..i + 2], 16).unwrap())
                .collect();
            for (mode, name) in [
                ("100644", "x/y"),
                ("100644", ".git"),
                ("100644", "."),
                ("100664", "x"),
                ("100600", "x"),
                ("120000", ".gitmodules"),
                ("100644", "git~1"),
                ("100644", ".GIT"),
                ("100644", "a\\.git"),
                ("120000", ".gitignore"),
            ] {
                let mut raw = format!("{mode} {name}\0").into_bytes();
                raw.extend_from_slice(&raw_oid);
                assert!(
                    !repo
                        .run_git(
                            &["hash-object", "-t", "tree", "--no-filters", "--stdin"],
                            Some(&raw)
                        )
                        .status
                        .success()
                );
                let nested = repo.get_oid(
                    &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                    Some(&raw),
                );
                let root = repo.get_oid(
                    &["mktree"],
                    Some(format!("040000 tree {nested}\tnested\n").as_bytes()),
                );
                let root = crate::domain::TreeOid::parse(&root).unwrap();
                // When: strong pin audits the nested closure.
                let error = git.pin(&root).unwrap_err();
                // Then: Git structural checks remain mandatory before publishing a ref.
                assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
                assert!(
                    !repo
                        .run_git(
                            &[
                                "show-ref",
                                "--verify",
                                &format!("{SNAPSHOT_REF_PREFIX}{}", root.as_str())
                            ],
                            None
                        )
                        .status
                        .success()
                );
            }
        }
    }

    #[test]
    fn discovers_root_from_subdirectory() {
        // Given
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let expected_length = match format {
                ObjectFormat::Sha1 => 40,
                ObjectFormat::Sha256 => 64,
            };
            // When
            let git = GitRepository::open(repo.get_subdir()).unwrap();
            // Then
            assert_eq!(git.root, repo.get_root());
            assert_eq!(git.oid_len, expected_length);
        }
    }

    #[test]
    fn open_rejects_non_repository_and_bare() {
        // Given
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let bare = repo.get_root().join("bare");
        repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
        let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
        fs::create_dir(&empty).unwrap();
        // When
        let bare_result = GitRepository::open(&bare);
        let empty_result = GitRepository::open(&empty);
        // Then
        assert!(bare_result.is_err());
        assert!(empty_result.is_err());
        fs::remove_dir(empty).unwrap();
    }

    #[test]
    fn discovery_rejects_git_metadata_as_a_working_directory() {
        // Given: Git metadata is inside the repository but outside its working tree.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        assert!(
            !repo
                .run_git(&["-C", ".git", "rev-parse", "--show-toplevel"], None)
                .status
                .success()
        );
        // When: discovery begins from the metadata directory itself.
        let result = GitRepository::open(&repo.get_root().join(".git"));
        // Then: the adapter preserves the working-copy-only boundary.
        assert_eq!(
            result.unwrap_err().get_kind(),
            SnapshotErrorKind::InvalidRequest
        );
    }

    #[test]
    fn discovers_trailing_newline_repository_without_touching_sibling() {
        // Given
        for format in fixture::FORMATS {
            let parent = TempRepo::new(format);
            let (sibling, target) = create_newline_repository_pair(&parent, format);
            let sibling_objects = parent.run_git_successfully(
                &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
                None,
            );
            let sibling_refs = parent
                .run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
            let path = RepoPath::parse("captured.md").unwrap();
            // When
            let git = open_git_snapshots(&target).unwrap();
            let tree = git
                .create_tree(&[SnapshotFile {
                    path: &path,
                    bytes: b"newline repository isolation\0",
                }])
                .unwrap();
            git.pin(&tree).unwrap();
            let objects_after = parent.run_git_successfully(
                &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
                None,
            );
            let refs_after = parent
                .run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
            let name = format!("{SNAPSHOT_REF_PREFIX}{}", tree.as_str());
            let actual = parent.run_git(
                &[
                    "-C",
                    target.to_str().unwrap(),
                    "show-ref",
                    "--verify",
                    &name,
                ],
                None,
            );
            let bytes = git.get_blob(&tree, &path).unwrap();
            let discovered_root = GitRepository::open(&target).unwrap().root;
            // Then
            assert_eq!(
                objects_after, sibling_objects,
                "opening repo\\n must not write objects into sibling repo"
            );
            assert_eq!(
                refs_after, sibling_refs,
                "opening repo\\n must not pin a ref in sibling repo"
            );
            assert!(
                actual.status.success(),
                "snapshot must be retained in the requested newline-suffixed repository"
            );
            assert_eq!(bytes, b"newline repository isolation\0");
            assert_eq!(discovered_root, target);
        }
    }

    #[test]
    fn rejects_non_absence_errors_when_inspecting_object_alternates() {
        // Given: ENOTDIR is deterministic even when tests run with elevated privileges.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let info = repo.get_root().join(".git/objects/info");
        fs::remove_dir(&info).unwrap();
        fs::write(&info, b"not a directory").unwrap();
        // When
        let error =
            GitRepository::open(repo.get_root()).expect_err("unsafe object store was accepted");
        // Then
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert_eq!(fs::read(info).unwrap(), b"not a directory");
    }

    #[test]
    fn root_verification_and_addressed_reads_reject_malformed_tree_structure() {
        use crate::{domain::TreeOid, service::SnapshotVerifier};
        // Given: a hash-correct tree with an invalid file mode, accepted only by --literally.
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
            let mut bytes = b"100664 x\0".to_vec();
            bytes.extend_from_slice(
                gix::hash::ObjectId::from_hex(blob.as_bytes())
                    .unwrap()
                    .as_bytes(),
            );
            let oid = repo.get_oid(
                &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                Some(&bytes),
            );
            let oid = TreeOid::parse(&oid).unwrap();
            let git = GitRepository::open(repo.get_root()).unwrap();
            // When: each operation reaches this root without a full closure audit.
            let verified = git.verify_snapshot(&oid).unwrap_err();
            let retained = git.retain(&oid).unwrap_err();
            let addressed = git
                .get_blob(&oid, &RepoPath::parse("x").unwrap())
                .unwrap_err();
            // Then: hash identity alone never makes malformed tree structure available.
            for error in [verified, retained, addressed] {
                assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            }
        }
    }

    fn create_newline_repository_pair(
        parent: &TempRepo,
        format: ObjectFormat,
    ) -> (PathBuf, PathBuf) {
        let sibling = parent.get_root().join("repo");
        let target = parent.get_root().join("repo\n");
        for root in [&sibling, &target] {
            parent.run_git_successfully(
                &["init", "-q", format.to_init_arg(), root.to_str().unwrap()],
                None,
            );
        }
        (sibling, target)
    }

    fn supervise_snapshot_formats() {
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let tree = repo.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            repo.run_git_successfully(&["gc", "--prune=now"], None);
            let before = repo.capture_fixture_state();
            fixture::supervise_test(
                "git::repository::tests::snapshots_work_without_git_executable",
                &[
                    ("LETOPIS_CHILD_FREE_ROOT", repo.get_root().to_str().unwrap()),
                    ("LETOPIS_CHILD_FREE_TREE", &tree),
                    ("PATH", "/nonexistent-letopis-git"),
                    ("GIT_DIR", "CANARY_DIR"),
                    ("GIT_WORK_TREE", "CANARY_TREE"),
                    ("GIT_INDEX_FILE", "CANARY_INDEX"),
                    ("GIT_OBJECT_DIRECTORY", "CANARY_OBJECTS"),
                    ("GIT_CONFIG_COUNT", "1"),
                    ("GIT_CONFIG_KEY_0", "core.bare"),
                    ("GIT_CONFIG_VALUE_0", "true"),
                    ("GIT_COMMON_DIR", "CANARY_COMMON"),
                    ("GIT_NAMESPACE", "CANARY_NAMESPACE"),
                    ("GIT_CONFIG_PARAMETERS", "CANARY_PARAMETERS"),
                    ("GIT_ALTERNATE_OBJECT_DIRECTORIES", "CANARY_ALTERNATES"),
                ],
            );
            assert_eq!(repo.capture_fixture_state(), before);
        }
    }
}
