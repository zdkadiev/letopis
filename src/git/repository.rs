use super::{
    cleanup::GitWriteCleanup,
    constants::{BLOB_KIND, FOREIGN_OID_FORMAT, OBJECT_INTEGRITY_MESSAGE, TREE_KIND},
    durability::{
        StagedObject, loose_object_get_by_content, loose_object_sync_by_oid, ref_lock_is_busy,
    },
    reader::{ObjectReader, VerifiedObject},
    tree::tree_entry_get,
    validation::tree_bytes_validate,
};
use crate::service::GitIgnoreMode;
use crate::{
    domain::{BlobOid, RepoPath, TreeOid},
    service::{
        GitError, GitResult, GitSnapshots, GitWorktreeListing, SnapshotEntry, SnapshotError,
        SnapshotErrorKind, SnapshotFile, SnapshotState, SnapshotVerifier,
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
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread,
    time::Duration,
};

#[cfg(test)]
use std::cell::Cell;

const MISSING_SNAPSHOT_PATH: &str = "snapshot path is missing";
const GITLINK_KIND: &str = "gitlink";
// Retry safe lock contention through the entire confirmation protocol; not a command deadline.
const REF_LOCK_ATTEMPTS: usize = 32;
const REF_RETRY_DELAY: Duration = Duration::from_millis(10);

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
        .map_err(repository_classify_open_error)?
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
        tree_create(self, files)
    }

    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        self.objects_verify_self_contained()?;
        blob_write(self, bytes)
    }

    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        self.objects_verify_self_contained()?;
        tree_assemble(self, entries)
    }

    fn blob_get_by_tree_oid_and_path(
        &self,
        tree: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.objects_verify_self_contained()?;
        blob_get_by_tree_oid_and_path(self, tree, path)
    }

    fn ref_update_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.objects_verify_self_contained()?;
        ref_update_by_oid(self, tree)
    }

    fn tree_sync_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.objects_verify_self_contained()?;
        tree_sync_by_oid(self, tree)
    }
}

impl SnapshotVerifier for GitRepository {
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.objects_verify_self_contained()?;
        match tree_validate_by_oid(self, tree) {
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

#[expect(
    clippy::needless_pass_by_value,
    reason = "map_err passes the error by value to the mapper"
)]
fn repository_classify_open_error(error: gix::Error) -> SnapshotError {
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

/// Creates a tree from exact snapshot file bytes after validating every path.
///
/// # Arguments
///
/// * `git` — repository whose excluded roots and object store are used.
/// * `files` — complete snapshot files whose exact bytes and paths form the tree.
///
/// # Errors
/// Reports invalid, duplicate or colliding paths and repository object failures.
fn tree_create(git: &GitRepository, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
    paths_validate(
        git.excluded_roots,
        &files.iter().map(|file| file.path).collect::<Vec<_>>(),
    )?;
    let entries = files
        .iter()
        .map(|file| Ok((file.path, blob_write(git, file.bytes)?)))
        .collect::<Result<Vec<_>, SnapshotError>>()?;
    tree_build(git, &entries)
}

/// Creates a tree from existing blobs after validating every path and each distinct blob.
///
/// # Arguments
///
/// * `git` — repository whose excluded roots, reader and object store are used.
/// * `entries` — snapshot entries whose declared paths and existing blobs form the tree.
///
/// # Errors
/// Reports invalid, duplicate or colliding paths, foreign identities, missing blobs and
/// repository object failures.
fn tree_assemble(git: &GitRepository, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
    paths_validate(
        git.excluded_roots,
        &entries.iter().map(|entry| &entry.path).collect::<Vec<_>>(),
    )?;
    let mut verified = BTreeSet::new();
    for entry in entries {
        if entry.oid.as_str().len() != git.oid_len {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                FOREIGN_OID_FORMAT,
            ));
        }
        if verified.insert(entry.oid.as_str()) {
            git.reader.borrow_mut().object_get_by_oid(
                entry.oid.as_str(),
                BLOB_KIND,
                SnapshotErrorKind::InvalidRequest,
                SnapshotErrorKind::InvalidRequest,
            )?;
        }
    }
    let entries: Vec<_> = entries
        .iter()
        .map(|entry| (&entry.path, entry.oid.clone()))
        .collect();
    tree_build(git, &entries)
}

/// Returns one blob's bytes after verifying the root and addressed objects without auditing
/// unrelated descendants.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — root tree identity whose addressed path is read.
/// * `path` — repository-relative path of the blob within the tree.
///
/// # Errors
/// Reports missing paths, non-blob leaves, foreign identities and object or decoding failures.
fn blob_get_by_tree_oid_and_path(
    git: &GitRepository,
    oid: &TreeOid,
    path: &RepoPath,
) -> Result<Vec<u8>, SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    let mut current = oid.as_str().to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        let bytes = tree_get_by_oid(
            git,
            &current,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        let (kind, oid) = tree_entry_get_by_name(git, &bytes, part.as_bytes())?;
        if parts.peek().is_none() {
            if kind != BLOB_KIND {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "snapshot path is not a blob",
                ));
            }
            return git.reader.borrow_mut().object_bytes_get_by_oid(
                &oid,
                BLOB_KIND,
                SnapshotErrorKind::InternalError,
                SnapshotErrorKind::InternalError,
            );
        }
        if kind != TREE_KIND {
            return Err(SnapshotError::new(
                SnapshotErrorKind::NotFound,
                MISSING_SNAPSHOT_PATH,
            ));
        }
        current = oid;
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::NotFound,
        MISSING_SNAPSHOT_PATH,
    ))
}

fn tree_entry_get_by_name(
    git: &GitRepository,
    mut bytes: &[u8],
    name: &[u8],
) -> Result<(&'static str, String), SnapshotError> {
    while !bytes.is_empty() {
        let (entry, rest) = tree_entry_get(bytes, git.oid_len)?;
        if entry.name == name {
            let kind = match entry.mode & 0o170000 {
                0o040000 => TREE_KIND,
                0o160000 => GITLINK_KIND,
                _ => BLOB_KIND,
            };
            let oid = ObjectId::try_from(entry.oid)
                .map_err(|_| {
                    SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
                })?
                .to_string();
            return Ok((kind, oid));
        }
        bytes = rest;
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::NotFound,
        MISSING_SNAPSHOT_PATH,
    ))
}

/// Verifies the root tree's hash, type and own structure without claiming closure integrity.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — tree identity whose hash, type and own structure are checked.
///
/// # Errors
/// Reports a foreign identity as an invalid request and missing or corrupt objects as snapshot
/// failures.
fn tree_validate_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    if oid.as_str().len() != git.oid_len {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InvalidRequest,
            FOREIGN_OID_FORMAT,
        ));
    }
    tree_get_by_oid(
        git,
        oid.as_str(),
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    )
    .map(|_| ())
}

/// Returns one verified tree body without traversing its children.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format are used.
/// * `oid` — hexadecimal object identity of the tree to read.
/// * `missing` — error kind used when the object is absent.
/// * `wrong` — error kind used when the object is not a tree.
///
/// # Errors
/// Returns the caller-selected missing or wrong-type error and reports decoding or integrity
/// failures as internal errors.
fn tree_get_by_oid(
    git: &GitRepository,
    oid: &str,
    missing: SnapshotErrorKind,
    wrong: SnapshotErrorKind,
) -> Result<Vec<u8>, SnapshotError> {
    git.reader
        .borrow_mut()
        .tree_bytes_get_by_oid(oid, missing, wrong)
}

fn tree_build(
    git: &GitRepository,
    entries: &[(&RepoPath, BlobOid)],
) -> Result<TreeOid, SnapshotError> {
    let mut root = Directory::default();
    for (path, oid) in entries {
        root.entry_insert_by_path(path.as_str(), oid.clone());
    }
    root.tree_write(git)
}

/// Verifies the root and durably confirms its exact direct ref without loading descendants.
///
/// # Arguments
///
/// * `git` — repository that verifies the root and updates the snapshot ref.
/// * `oid` — identity of the root tree to validate and update its ref.
///
/// # Errors
/// Reports an invalid or missing root and ref lock, write, sync or confirmation failures.
fn ref_update_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    ref_upsert_by_oid(git, oid)
}

/// Verifies the entire closure on every call and then updates the snapshot ref.
///
/// # Arguments
///
/// * `git` — repository that verifies the closure and updates the snapshot ref.
/// * `oid` — identity of the root tree to verify and update its ref.
///
/// # Errors
/// Reports an invalid or missing root, a corrupt reachable object or closure type conflict,
/// and ref lock, write, sync or confirmation failures.
fn tree_sync_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    tree_verify_closure_by_oid(git, oid)?;
    ref_upsert_by_oid(git, oid)?;
    Ok(())
}

fn ref_upsert_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    let oid = ObjectId::from_hex(oid.as_str().as_bytes()).map_err(|_| {
        SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git identity")
    })?;
    for attempt in 0..REF_LOCK_ATTEMPTS {
        match super::durability::ref_update_by_oid(&git.repo, oid, &git.cleanup) {
            Err(error) if ref_lock_is_busy(&error) && attempt + 1 < REF_LOCK_ATTEMPTS => {
                thread::sleep(REF_RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("bounded retry always returns on the last attempt")
}

/// Verifies every object reachable from `root`, ignoring gitlinks, and rejects type conflicts.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format verify the closure.
/// * `root` — identity of the root tree whose reachable objects are checked.
///
/// # Errors
/// Reports missing or corrupt reachable objects and conflicting object types.
fn tree_verify_closure_by_oid(git: &GitRepository, root: &TreeOid) -> Result<(), SnapshotError> {
    let mut reader = git.reader.borrow_mut();
    let mut pending = vec![(root.as_str().to_owned(), TREE_KIND.to_owned())];
    let mut visited = BTreeMap::new();
    while let Some((oid, kind)) = pending.pop() {
        if let Some(previous) = visited.insert(oid.clone(), kind.clone()) {
            if previous != kind {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "conflicting Git object types",
                ));
            }
            continue;
        }
        let object = reader.object_get_by_oid(
            &oid,
            &kind,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        if let VerifiedObject::Tree(bytes) = object {
            tree_bytes_validate(&bytes, git.oid_len)?;
            pending.extend(tree_children_get(&bytes, git.oid_len)?);
        }
    }
    Ok(())
}

/// Returns the direct child object identities and kinds from a verified tree body.
///
/// # Arguments
///
/// * `bytes` — verified tree body whose direct children are decoded.
/// * `oid_len` — hexadecimal object ID length used to frame each entry.
///
/// # Errors
/// Returns `InternalError` for malformed entry boundaries or octal modes.
fn tree_children_get(
    mut bytes: &[u8],
    oid_len: usize,
) -> Result<Vec<(String, String)>, SnapshotError> {
    let mut children = Vec::new();
    while !bytes.is_empty() {
        let (entry, remaining) = tree_entry_get(bytes, oid_len)?;
        let kind = match entry.mode & 0o170000 {
            0o100000 | 0o120000 => Some(BLOB_KIND),
            0o040000 => Some(TREE_KIND),
            _ => None,
        };
        if let Some(kind) = kind {
            let oid: String = entry.oid.iter().map(|byte| format!("{byte:02x}")).collect();
            children.push((oid, kind.into()));
        }
        bytes = remaining;
    }
    Ok(children)
}

#[derive(Default)]
struct Directory<'a> {
    files: BTreeMap<&'a str, BlobOid>,
    dirs: BTreeMap<&'a str, Directory<'a>>,
}

impl<'a> Directory<'a> {
    fn entry_insert_by_path(&mut self, path: &'a str, oid: BlobOid) {
        match path.split_once('/') {
            Some((dir, rest)) => self
                .dirs
                .entry(dir)
                .or_default()
                .entry_insert_by_path(rest, oid),
            None => {
                self.files.insert(path, oid);
            }
        }
    }

    fn tree_write(&self, git: &GitRepository) -> Result<TreeOid, SnapshotError> {
        // Git compares directory names with a virtual '/' terminator.
        let mut entries = BTreeMap::new();
        for (name, oid) in &self.files {
            entries.insert(
                (*name).to_owned(),
                ("100644", *name, oid.as_str().to_owned()),
            );
        }
        for (name, dir) in &self.dirs {
            entries.insert(
                format!("{name}/"),
                ("40000", *name, dir.tree_write(git)?.as_str().to_owned()),
            );
        }
        let mut bytes = Vec::new();
        for (mode, name, oid) in entries.values() {
            bytes.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            let oid = ObjectId::from_hex(oid.as_bytes()).map_err(|_| {
                SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
            })?;
            bytes.extend_from_slice(oid.as_bytes());
        }
        tree_bytes_validate(&bytes, git.oid_len)?;
        TreeOid::parse(&object_write(git, gix::objs::Kind::Tree, &bytes)?).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
        })
    }
}

/// Writes exact bytes durably without filters or newline normalization.
///
/// # Arguments
///
/// * `git` — repository whose object store receives the blob.
/// * `bytes` — exact payload written without filters or newline normalization.
///
/// # Errors
/// Reports object identity, staging, synchronization or publication failures.
fn blob_write(git: &GitRepository, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
    BlobOid::parse(&object_write(git, gix::objs::Kind::Blob, bytes)?)
        .map_err(|_| SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE))
}

fn object_write(
    git: &GitRepository,
    kind: gix::objs::Kind,
    bytes: &[u8],
) -> Result<String, SnapshotError> {
    if let Some(oid) = loose_object_get_by_content(&git.repo, kind, bytes, &git.cleanup)? {
        loose_object_sync_by_oid(&git.repo, oid, &git.cleanup)?;
        return Ok(oid.to_string());
    }
    let object = StagedObject::new_with_cleanup(&git.repo, kind, bytes, &git.cleanup)?;
    let oid = object.object_get_oid().to_string();
    object.object_publish()?;
    Ok(oid)
}

/// Rejects duplicate, colliding, Git-metadata and excluded-root snapshot paths.
///
/// # Arguments
///
/// * `excluded_roots` — root-relative names that a snapshot path must not start with.
/// * `paths` — complete snapshot paths checked as one set.
///
/// # Errors
/// Reports invalid, duplicate or file-directory colliding paths as invalid requests.
fn paths_validate(excluded_roots: &[&str], paths: &[&RepoPath]) -> Result<(), SnapshotError> {
    let mut unique = BTreeSet::new();
    for path in paths {
        let path = path.as_str();
        let mut parts = path.split('/');
        if parts.clone().any(|part| part.eq_ignore_ascii_case(".git"))
            || parts
                .next()
                .is_some_and(|root| excluded_roots.contains(&root))
            || !unique.insert(path)
        {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "invalid snapshot paths",
            ));
        }
    }
    for path in &unique {
        for (offset, character) in path.char_indices() {
            if character == '/' && unique.contains(&path[..offset]) {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InvalidRequest,
                    "snapshot file and directory collide",
                ));
            }
        }
    }
    Ok(())
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
