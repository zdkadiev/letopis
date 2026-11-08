use super::{
    cleanup::GitWriteCleanup, objects, objects::tree_get_by_oid, reader::ObjectReader, refs,
    tree::tree_entry_get,
};
use crate::service::GitIgnoreMode;
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{
        GitError, GitResult, GitSnapshots, GitWorktreeListing, RetainedTree, SnapshotEntry,
        SnapshotError, SnapshotErrorKind, SnapshotFile, SnapshotState, SnapshotVerifier,
    },
};
use gix::{
    bstr::ByteSlice,
    glob::pattern::Case,
    hash::ObjectId,
    ignore::Search as IgnoreSearch,
    index::{
        State,
        entry::{Flags, Mode},
    },
};
use gix_dir::{
    entry::{Kind, Status},
    walk::{Context, Options, delegate::Collect},
};
use gix_pathspec::Search;
use std::{
    borrow::Cow,
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
    /// Opens a self-contained worktree without executable helpers or environment routing.
    ///
    /// # Arguments
    ///
    /// * `start_path` — path where the command was invoked; discovery walks to its work tree.
    /// * `excluded_roots` — service directories that must stay outside the repository corpus.
    ///
    /// # Errors
    /// Reports unsupported repositories, unsafe service roots or Git open failures.
    pub(crate) fn open(
        start_path: &Path,
        excluded_roots: &'static [&'static str],
    ) -> Result<Self, SnapshotError> {
        Self::open_with_cleanup(start_path, Arc::new(GitWriteCleanup::new()), excluded_roots)
    }

    /// Opens a worktree sharing the command's write-resource owner with the signal callback.
    ///
    /// # Arguments
    ///
    /// * `start_path` — path where the command was invoked; discovery walks to its work tree.
    /// * `cleanup` — write resources shared with the foreground cancellation path.
    /// * `excluded_roots` — service directories that must stay outside the repository corpus.
    ///
    /// # Errors
    /// Reports unsupported repositories, unsafe service roots or Git open failures.
    pub(crate) fn open_with_cleanup(
        start_path: &Path,
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
            start_path,
            Default::default(),
            gix::sec::trust::Mapping {
                full: options.clone(),
                reduced: options,
            },
        )
        .map_err(|error| repository_classify_open_error(&error))?
        .to_thread_local();
        worktree_verify_start_path(&repo, start_path)?;
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
        alternates_verify_absent(&alternates)?;
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

    /// Returns the number of discovery calls on this test thread, excluding clones.
    #[cfg(test)]
    pub(crate) fn repository_get_open_count() -> usize {
        REPOSITORY_OPEN_COUNT.with(Cell::get)
    }

    /// Returns the discovered worktree root.
    pub(crate) fn worktree_root_get(&self) -> &Path {
        &self.root
    }

    /// Verifies that the repository has no object alternates on every operation.
    pub(super) fn objects_verify_self_contained(&self) -> Result<(), SnapshotError> {
        alternates_verify_absent(&self.alternates)
    }
}

impl GitSnapshots for GitRepository {
    fn tree_create(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        self.objects_verify_self_contained()?;
        objects::tree_create(self, files)
    }

    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        self.objects_verify_self_contained()?;
        objects::blob_write(self, bytes)
    }

    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        self.objects_verify_self_contained()?;
        objects::tree_assemble(self, entries)
    }

    fn blob_get_by_tree_oid_and_path(
        &self,
        tree: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.objects_verify_self_contained()?;
        objects::blob_get_by_tree_oid_and_path(self, tree, path)
    }

    fn tree_retain_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.objects_verify_self_contained()?;
        refs::tree_retain_by_oid(self, tree)
    }

    fn tree_pin_by_oid(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        self.objects_verify_self_contained()?;
        refs::tree_pin_by_oid(self, tree)
    }
}

impl SnapshotVerifier for GitRepository {
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.objects_verify_self_contained()?;
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

impl GitWorktreeListing for GitRepository {
    fn worktree_paths_get_by_ignore_mode_and_patterns(
        &self,
        ignore_mode: GitIgnoreMode,
        patterns: &[u8],
    ) -> GitResult<Vec<u8>> {
        worktree_paths_get_by_ignore_mode_and_patterns(self, &self.root, ignore_mode, patterns)
    }
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

/// Rejects a start path that resolves inside Git metadata instead of the working copy.
///
/// # Arguments
///
/// * `repo` — opened repository whose metadata directories are compared.
/// * `start_path` — canonicalized command invocation path to verify.
///
/// # Errors
/// Reports unreadable paths or a start path inside repository metadata.
fn worktree_verify_start_path(
    repo: &gix::Repository,
    start_path: &Path,
) -> Result<(), SnapshotError> {
    let start_path = fs::canonicalize(start_path).map_err(|_| {
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
        if start_path.starts_with(metadata) {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "Git working copy required",
            ));
        }
    }
    Ok(())
}

fn alternates_verify_absent(path: &Path) -> Result<(), SnapshotError> {
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

/// Returns index and untracked paths after Git ignore filtering.
///
/// Steps:
/// 1. Expand sparse-index metadata in a private copy, so excludes and the walk observe one
///    tracked view without touching the user's index.
/// 2. Collect every index entry path, retaining deleted tracked paths and duplicate stages.
/// 3. Load the repository exclude sources only when `ignore_mode` requests standard Git rules.
/// 4. Walk the worktree under `root`, skipping ignored entries and Git metadata, and append each
///    untracked regular file and symlink as a NUL-terminated path.
/// 5. Apply `patterns` to the complete union, using the repository's case mode.
///
/// The result holds nonempty NUL-terminated paths in walk/index order; duplicates are preserved.
///
/// # Arguments
///
/// * `git` — opened repository whose index, objects and configuration are read.
/// * `root` — canonical worktree root used as the walk boundary.
/// * `ignore_mode` — selects standard Git exclusion sources or their absence.
/// * `patterns` — root-relative Gitignore bytes applied to the complete path union.
///
/// # Errors
/// Reports index, ignore, sparse-object, worktree traversal or repository-policy failures.
fn worktree_paths_get_by_ignore_mode_and_patterns(
    git: &GitRepository,
    root: &Path,
    ignore_mode: GitIgnoreMode,
    patterns: &[u8],
) -> GitResult<Vec<u8>> {
    let repo = &git.repo;
    let index = repo.index_or_empty().map_err(GitError::new)?;
    let index = index_expand_sparse(git, &index)?;
    let mut raw = index_paths_get(&index);
    let mut excludes = if ignore_mode == GitIgnoreMode::Standard {
        Some(
            repo.excludes(&index, None, Default::default())
                .map_err(GitError::new)?
                .detach(),
        )
    } else {
        None
    };
    let mut pathspec = Search::from_specs(std::iter::empty(), None, root).map_err(GitError::new)?;
    let caps = repo.filesystem_options().map_err(GitError::new)?;
    let lookup = caps.ignore_case.then(|| index.prepare_icase_backing());
    let git_dir = repo.git_dir().canonicalize().map_err(GitError::new)?;
    let mut found = Collect::default();
    gix_dir::walk(
        root,
        Context {
            should_interrupt: None,
            git_dir_realpath: &git_dir,
            current_dir: root,
            index: &index,
            ignore_case_index_lookup: lookup.as_ref(),
            pathspec: &mut pathspec,
            pathspec_attributes: &mut |_, _, _, _| false,
            excludes: excludes.as_mut(),
            objects: &repo.objects,
            explicit_traversal_root: Some(root),
        },
        Options {
            ignore_case: caps.ignore_case,
            precompose_unicode: caps.precompose_unicode,
            ..Options::default()
        },
        &mut found,
    )
    .map_err(GitError::new)?;
    for (entry, _) in found.unorded_entries {
        if entry.status == Status::Untracked
            && matches!(entry.disk_kind, Some(Kind::File | Kind::Symlink))
        {
            raw.extend_from_slice(&entry.rela_path);
            raw.push(0);
        }
    }
    paths_filter_by_patterns(
        &raw,
        patterns,
        if caps.ignore_case {
            Case::Fold
        } else {
            Case::Sensitive
        },
    )
}

/// Expands only metadata in a private copy so excludes and the walk use the same tracked view.
///
/// # Arguments
///
/// * `git` — repository whose self-containment policy and objects supply missing entries.
/// * `index` — index state whose sparse entries are expanded in a private copy.
fn index_expand_sparse<'a>(git: &GitRepository, index: &'a State) -> GitResult<Cow<'a, State>> {
    if !index.is_sparse() {
        return Ok(Cow::Borrowed(index));
    }
    git.objects_verify_self_contained().map_err(GitError::new)?;
    let mut pending: Vec<_> = index
        .entries()
        .iter()
        .filter(|entry| entry.mode.is_sparse())
        .map(|entry| (entry.path(index).to_vec(), entry.id, entry.mode))
        .collect();
    let mut expanded = index.clone();
    expanded.remove_entries(|_, _, entry| entry.mode.is_sparse());
    while let Some((path, id, mode)) = pending.pop() {
        if mode.is_sparse() {
            pending.extend(tree_children_get_by_prefix_and_oid(git, &path, id)?);
            continue;
        }
        expanded.dangerously_push_entry(
            Default::default(),
            id,
            Flags::SKIP_WORKTREE,
            mode,
            path.as_bstr(),
        );
    }
    expanded.sort_entries();
    Ok(Cow::Owned(expanded))
}

/// Returns the direct children of a tree, keeping only child metadata between reads.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format decode the tree.
/// * `prefix` — repository-relative path prefix prepended to each returned child path.
/// * `oid` — identity of the tree whose direct children are returned.
fn tree_children_get_by_prefix_and_oid(
    git: &GitRepository,
    prefix: &[u8],
    oid: ObjectId,
) -> GitResult<Vec<(Vec<u8>, ObjectId, Mode)>> {
    let bytes = tree_get_by_oid(
        git,
        &oid.to_string(),
        SnapshotErrorKind::InternalError,
        SnapshotErrorKind::InternalError,
    )
    .map_err(GitError::new)?;
    let mut remaining = bytes.as_slice();
    let mut children = Vec::new();
    while !remaining.is_empty() {
        let (entry, rest) = tree_entry_get(remaining, git.oid_len).map_err(GitError::new)?;
        let mut path = prefix.to_vec();
        path.extend_from_slice(entry.name);
        let id = ObjectId::try_from(entry.oid).map_err(GitError::new)?;
        let mode = Mode::from_bits(u32::from(entry.mode))
            .ok_or_else(|| GitError::new(std::io::Error::other("invalid Git index mode")))?;
        if mode.is_sparse() {
            path.push(b'/');
        }
        children.push((path, id, mode));
        remaining = rest;
    }
    Ok(children)
}

fn index_paths_get(index: &State) -> Vec<u8> {
    let mut raw = Vec::new();
    for entry in index.entries() {
        raw.extend_from_slice(entry.path(index));
        raw.push(0);
    }
    raw
}

/// Filters NUL-framed paths by root-relative patterns without interpreting document roles.
///
/// # Arguments
///
/// * `raw` — NUL-terminated repository paths, including tracked and deleted entries.
/// * `patterns` — root-relative Gitignore bytes; the parser skips invalid lines.
/// * `case` — repository case policy applied to pattern matching.
///
/// # Errors
/// Reports a pattern buffer that cannot be attached to the fresh in-memory search.
fn paths_filter_by_patterns(raw: &[u8], patterns: &[u8], case: Case) -> GitResult<Vec<u8>> {
    let mut search = IgnoreSearch::default();
    search
        .add_patterns_buffer(patterns, "ignore", None, Default::default())
        .map_err(GitError::new)?;
    let mut filtered = Vec::new();
    for path in raw.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        if !path_is_excluded(&search, path, case) {
            filtered.extend_from_slice(path);
            filtered.push(0);
        }
    }
    Ok(filtered)
}

/// Returns whether a lexical ancestor or the leaf itself matches an excluding pattern.
///
/// # Arguments
///
/// * `search` — root-relative pattern set attached to this enumeration.
/// * `path` — repository-relative path bytes without a trailing separator.
/// * `case` — repository case policy applied to pattern matching.
fn path_is_excluded(search: &IgnoreSearch, path: &[u8], case: Case) -> bool {
    for (index, _) in path.iter().enumerate().filter(|(_, byte)| **byte == b'/') {
        if search
            .pattern_matching_relative_path(path[..index].as_bstr(), Some(true), case)
            .is_some_and(|matched| !matched.pattern.is_negative())
        {
            return true;
        }
    }
    search
        .pattern_matching_relative_path(path.as_bstr(), Some(false), case)
        .is_some_and(|matched| !matched.pattern.is_negative())
}

#[cfg(test)]
#[path = "repository_test.rs"]
mod tests;
