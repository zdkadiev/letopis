use super::{cleanup::GitWriteCleanup, objects, reader::ObjectReader, refs, worktree};
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{
        GitResult, GitSnapshots, GitWorktreePaths, RetainedTree, SnapshotEntry, SnapshotError,
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
    pub(super) excluded_roots: &'static [&'static str],
}
impl GitRepository {
    /// Discover a self-contained worktree without executable helpers or environment routing.
    pub(crate) fn open(
        start: &Path,
        excluded_roots: &'static [&'static str],
    ) -> Result<Self, SnapshotError> {
        Self::open_with_cleanup(start, Arc::new(GitWriteCleanup::new()), excluded_roots)
    }
    /// Share the command's actual write-resource owner with the signal callback.
    pub(crate) fn open_with_cleanup(
        start: &Path,
        cleanup: Arc<GitWriteCleanup>,
        excluded_roots: &'static [&'static str],
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
        .map_err(|error| repository_classify_open_error(&error))?
        .to_thread_local();
        worktree_require_start(&repo, start)?;
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
        alternates_require_absent(&alternates)?;
        let reader = Rc::new(RefCell::new(ObjectReader::new(&repo, oid_len)?));
        Ok(Self {
            repo: Rc::new(repo),
            reader,
            root,
            oid_len,
            cleanup,
            alternates,
            excluded_roots,
        })
    }
    /// Count actual library discovery calls on this test thread, excluding clones.
    #[cfg(test)]
    pub(crate) fn repository_get_open_count() -> usize {
        REPOSITORY_OPEN_COUNT.with(Cell::get)
    }
    /// Return the discovered worktree root.
    pub(crate) fn worktree_root_get(&self) -> &Path {
        &self.root
    }
    /// Recheck policy at every operation; an earlier successful open is not authority.
    pub(super) fn objects_require_self_contained(&self) -> Result<(), SnapshotError> {
        alternates_require_absent(&self.alternates)
    }
}
impl GitSnapshots for GitRepository {
    fn tree_retain_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.objects_require_self_contained()?;
        refs::tree_retain_by_oid(self, tree)
    }
    fn tree_create(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        self.objects_require_self_contained()?;
        objects::tree_create(self, files)
    }
    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        self.objects_require_self_contained()?;
        objects::blob_write(self, bytes)
    }
    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        self.objects_require_self_contained()?;
        objects::tree_assemble(self, entries)
    }
    fn blob_get_by_tree_oid_and_path(
        &self,
        tree: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.objects_require_self_contained()?;
        objects::blob_get_by_tree_oid_and_path(self, tree, path)
    }
    fn tree_pin_by_oid(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        self.objects_require_self_contained()?;
        refs::tree_pin_by_oid(self, tree)
    }
}

impl SnapshotVerifier for GitRepository {
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.objects_require_self_contained()?;
        match objects::tree_validate_by_oid(self, tree) {
            Ok(()) => Ok(SnapshotState::Available),
            Err(error) => match error.get_kind() {
                SnapshotErrorKind::NotFound => Ok(SnapshotState::Missing),
                SnapshotErrorKind::InvalidRequest => Ok(SnapshotState::Invalid),
                SnapshotErrorKind::InternalError => Err(error),
            },
        }
    }
}

impl GitWorktreePaths for GitRepository {
    fn worktree_paths_get(&self) -> GitResult<Vec<u8>> {
        worktree::worktree_paths_get(self, &self.root)
    }
}
fn alternates_require_absent(path: &Path) -> Result<(), SnapshotError> {
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
fn worktree_require_start(repo: &gix::Repository, start: &Path) -> Result<(), SnapshotError> {
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
fn repository_classify_open_error(error: &gix::Error) -> SnapshotError {
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

    #[test]
    fn worktree_paths_preserve_raw_names_and_non_document_entries() {
        use crate::service::GitWorktreePaths;
        // Given: an index containing a non-UTF-8 Markdown path and an ordinary text file.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut entry = format!("100644 {oid}\t").into_bytes();
        entry.extend_from_slice(b"bad\xff.md\0");
        repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
        fs::write(repo.get_root().join("plain.txt"), b"text").unwrap();
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        // When: the infrastructure boundary enumerates repository paths.
        let paths = git.worktree_paths_get();
        // Then: document encoding and extension policies do not reject Git paths.
        let paths = paths.unwrap();
        assert!(
            paths
                .split(|byte| *byte == 0)
                .any(|path| path == b"bad\xff.md")
        );
        assert!(
            paths
                .split(|byte| *byte == 0)
                .any(|path| path == b"plain.txt")
        );
        assert!(paths.ends_with(&[0]));
    }

    #[test]
    fn repository_open_counter_excludes_clones() {
        // Given: an independently prepared repository and this thread's open count.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let before = GitRepository::repository_get_open_count();
        // When: one discovery is followed by a cheap clone and an independent open.
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let shared = git.clone();
        // Then: clones share both the repository and reader without discovering again.
        assert_eq!(GitRepository::repository_get_open_count() - before, 1);
        assert!(Rc::ptr_eq(&git.repo, &shared.repo));
        assert!(Rc::ptr_eq(&git.reader, &shared.reader));
        assert!(Arc::ptr_eq(&git.cleanup, &shared.cleanup));
        // When: an independent caller opens the same worktree.
        let independent =
            GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        // Then: it owns fresh state and performs its own discovery.
        assert_eq!(GitRepository::repository_get_open_count() - before, 2);
        assert!(!Rc::ptr_eq(&git.repo, &independent.repo));
    }

    #[test]
    fn assembly_checks_unique_oids_once_without_weakening_supplied_or_final_validation() {
        use crate::service::SnapshotEntry;
        for format in fixture::FORMATS {
            // Given: one independently written blob supplied under three distinct paths.
            let repo = TempRepo::new(format);
            let git =
                GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
            let oid = git.blob_write(b"original bytes").unwrap();
            let entries = ["a.md", "b.md", "nested/c.md"].map(|path| SnapshotEntry {
                path: RepoPath::parse(path).unwrap(),
                oid: oid.clone(),
            });
            let before = git.reader.borrow().object_get_read_count();
            // When: assemble from repeated caller-supplied object identities.
            let tree = git.tree_assemble(&entries).unwrap();
            // Then: exactly one addressed verification occurs in this assembly.
            assert_eq!(git.reader.borrow().object_get_read_count() - before, 1);
            git.tree_pin_by_oid(&tree).unwrap();
            // Given: valid compressed bytes replace that blob under its old identity.
            let other = git.blob_write(b"corrupt substitute").unwrap();
            repo.replace_loose_object(oid.as_str(), other.as_str());
            // When / Then: neither another assembly nor strong pin trusts the prior success/ref.
            assert_eq!(
                git.tree_assemble(&entries).unwrap_err().get_kind(),
                SnapshotErrorKind::InternalError
            );
            assert_eq!(
                git.tree_pin_by_oid(&tree).unwrap_err().get_kind(),
                SnapshotErrorKind::InternalError
            );
        }
    }

    #[test]
    fn snapshots_work_without_git_executable() {
        use crate::{
            domain::TreeOid,
            service::{GitWorktreePaths, SnapshotState, SnapshotVerifier},
        };
        // Given: historical packed objects, prepared with an independent Git executable.
        let Ok(root) = std::env::var("LETOPIS_CHILD_FREE_ROOT") else {
            supervise_snapshot_formats();
            return;
        };
        let tree = TreeOid::parse(&std::env::var("LETOPIS_CHILD_FREE_TREE").unwrap()).unwrap();
        // When: every snapshot operation runs with an empty executable search path.
        let git = GitRepository::open(
            std::path::Path::new(&root),
            &[crate::utils::SERVICE_DIRECTORY],
        )
        .unwrap();
        let path = RepoPath::parse("exact.md").unwrap();
        let blob = git.blob_write(b"raw\r\n\0bytes").unwrap();
        let captured = git
            .tree_assemble(&[crate::service::SnapshotEntry {
                path: path.clone(),
                oid: blob,
            }])
            .unwrap();
        let empty = git.tree_create(&[]).unwrap();
        git.tree_retain_by_oid(&tree).unwrap();
        git.tree_pin_by_oid(&tree).unwrap();
        git.tree_pin_by_oid(&captured).unwrap();
        // Then: old and new snapshots remain usable, with exact bytes and raw worktree path membership.
        assert_eq!(
            git.tree_verify_by_oid(&tree).unwrap(),
            SnapshotState::Available
        );
        assert_eq!(
            git.tree_verify_by_oid(&empty).unwrap(),
            SnapshotState::Available
        );
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(&captured, &path).unwrap(),
            b"raw\r\n\0bytes"
        );
        assert!(
            git.worktree_paths_get()
                .unwrap()
                .split(|byte| *byte == 0)
                .any(|path| path == b"live.md")
        );
    }

    #[test]
    fn strong_pin_rejects_malformed_nested_trees_like_git() {
        // Given: valid roots wrap malformed nested trees written only with --literally.
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let git =
                GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
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
                let error = git.tree_pin_by_oid(&root).unwrap_err();
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
        // Given: SHA-1 and SHA-256 repositories contain a subdirectory.
        for format in fixture::FORMATS {
            let repo = TempRepo::new(format);
            let expected_length = match format {
                ObjectFormat::Sha1 => 40,
                ObjectFormat::Sha256 => 64,
            };
            // When: open each repository from its subdirectory.
            let git =
                GitRepository::open(repo.get_subdir(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
            // Then: discovery returns the repository root and matching OID length.
            assert_eq!(git.root, repo.get_root());
            assert_eq!(git.oid_len, expected_length);
        }
    }

    #[test]
    fn open_rejects_non_repository_and_bare() {
        // Given: a bare repository and an ordinary empty directory.
        let repo = TempRepo::new(ObjectFormat::Sha1);
        let bare = repo.get_root().join("bare");
        repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
        let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
        fs::create_dir(&empty).unwrap();
        // When: attempt to open both paths as working repositories.
        let bare_result = GitRepository::open(&bare, &[crate::utils::SERVICE_DIRECTORY]);
        let empty_result = GitRepository::open(&empty, &[crate::utils::SERVICE_DIRECTORY]);
        // Then: both paths are rejected.
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
        let result = GitRepository::open(
            &repo.get_root().join(".git"),
            &[crate::utils::SERVICE_DIRECTORY],
        );
        // Then: the adapter preserves the working-copy-only boundary.
        assert_eq!(
            result.unwrap_err().get_kind(),
            SnapshotErrorKind::InvalidRequest
        );
    }

    #[test]
    fn discovers_trailing_newline_repository_without_touching_sibling() {
        // Given: sibling repositories share a parent and the target name ends with a newline.
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
            // When: create, retain, and read a snapshot in the target repository.
            let git = open_git_snapshots(&target).unwrap();
            let tree = git
                .tree_create(&[SnapshotFile {
                    path: &path,
                    bytes: b"newline repository isolation\0",
                }])
                .unwrap();
            git.tree_pin_by_oid(&tree).unwrap();
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
            let bytes = git.blob_get_by_tree_oid_and_path(&tree, &path).unwrap();
            let discovered_root = GitRepository::open(&target, &[crate::utils::SERVICE_DIRECTORY])
                .unwrap()
                .root;
            // Then: the target root and bytes are correct, and sibling objects and refs are unchanged.
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
        // When: open the repository whose objects/info path is a file.
        let error = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY])
            .expect_err("unsafe object store was accepted");
        // Then: opening reports InternalError and preserves the file bytes.
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
            let git =
                GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
            // When: each operation reaches this root without a full closure audit.
            let verified = git.tree_verify_by_oid(&oid).unwrap_err();
            let retained = git.tree_retain_by_oid(&oid).unwrap_err();
            let addressed = git
                .blob_get_by_tree_oid_and_path(&oid, &RepoPath::parse("x").unwrap())
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
