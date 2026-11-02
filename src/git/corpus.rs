use super::{GitRepository, objects::ObjectDb, tree::get_entry};
use crate::{
    domain::RepoPath,
    service::{DocumentError, DocumentErrorKind, DocumentResult, SnapshotErrorKind},
    utils::SERVICE_DIRECTORY,
};
use gix::{
    bstr::ByteSlice,
    hash::ObjectId,
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
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, FileType, Mode as FileMode, OFlags, open, openat, statat},
    io::Errno,
};
use std::{borrow::Cow, collections::BTreeSet, ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// List Markdown candidates from the index and non-ignored untracked files.
pub(super) fn get_candidates(git: &GitRepository, root: &Path) -> DocumentResult<Vec<RepoPath>> {
    let repo = &git.repo;
    let index = repo.index_or_empty().map_err(|_| fail_listing())?;
    let index = expand_sparse_index(git, &index)?;
    let mut raw = Vec::new();
    for entry in index.entries() {
        raw.extend_from_slice(entry.path(&index));
        raw.push(0);
    }
    let mut excludes = repo
        .excludes(&index, None, Default::default())
        .map_err(|_| fail_listing())?
        .detach();
    let mut pathspec =
        Search::from_specs(std::iter::empty(), None, root).map_err(|_| fail_listing())?;
    let caps = repo.filesystem_options().map_err(|_| fail_listing())?;
    let lookup = caps.ignore_case.then(|| index.prepare_icase_backing());
    let git_dir = repo.git_dir().canonicalize().map_err(|_| fail_listing())?;
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
            excludes: Some(&mut excludes),
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
    .map_err(|_| fail_listing())?;
    for (entry, _) in found.unorded_entries {
        if entry.status == Status::Untracked
            && matches!(entry.disk_kind, Some(Kind::File | Kind::Symlink))
        {
            raw.extend_from_slice(&entry.rela_path);
            raw.push(0);
        }
    }
    let filtered = filter_candidates(root, &raw)?;
    parse_candidates(&filtered)
}

fn filter_candidates(root: &Path, raw: &[u8]) -> DocumentResult<Vec<u8>> {
    let root = open(root, DIRECTORY_FLAGS, FileMode::empty()).map_err(|_| fail_listing())?;
    let mut filtered = Vec::new();
    for entry in raw
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let eligible = is_corpus_entry(entry);
        let safe_components = entry
            .split(|byte| *byte == b'/')
            .all(|part| !part.is_empty() && part != b"." && part != b"..");
        if eligible && safe_components && is_symlink(&root, entry).map_err(|_| fail_listing())? {
            continue;
        }
        filtered.extend_from_slice(entry);
        filtered.push(0);
    }
    Ok(filtered)
}

/// Check the live entry without following any component; absent tracked paths remain candidates.
fn is_symlink(root: &OwnedFd, path: &[u8]) -> Result<bool, Errno> {
    let mut parent = openat(root, ".", DIRECTORY_FLAGS, FileMode::empty())?;
    let mut components = path.split(|byte| *byte == b'/').peekable();
    while let Some(component) = components.next() {
        let name = OsStr::from_bytes(component);
        if components.peek().is_none() {
            return match statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => Ok(FileType::from_raw_mode(stat.st_mode) == FileType::Symlink),
                Err(Errno::NOENT) => Ok(false),
                Err(error) => Err(error),
            };
        }
        parent = match openat(&parent, name, DIRECTORY_FLAGS, FileMode::empty()) {
            Ok(directory) => directory,
            Err(Errno::NOENT) => return Ok(false),
            Err(error) => return Err(error),
        };
    }
    Err(Errno::INVAL)
}

/// Expand only metadata in a private copy: excludes and walk need the same tracked view.
fn expand_sparse_index<'a>(
    git: &GitRepository,
    index: &'a State,
) -> DocumentResult<Cow<'a, State>> {
    if !index.is_sparse() {
        return Ok(Cow::Borrowed(index));
    }
    git.require_self_contained_objects()
        .map_err(|_| fail_listing())?;
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
            pending.extend(get_sparse_children(git, &path, id)?);
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
/// Keep only child metadata between reads; verified tree payloads never accumulate.
fn get_sparse_children(
    git: &GitRepository,
    prefix: &[u8],
    oid: ObjectId,
) -> DocumentResult<Vec<(Vec<u8>, ObjectId, Mode)>> {
    let bytes = ObjectDb { git }
        .get_tree(
            &oid.to_string(),
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )
        .map_err(|_| fail_listing())?;
    let mut remaining = bytes.as_slice();
    let mut children = Vec::new();
    while !remaining.is_empty() {
        let (entry, rest) = get_entry(remaining, git.oid_len).map_err(|_| fail_listing())?;
        let mut path = prefix.to_vec();
        path.extend_from_slice(entry.name);
        let id = ObjectId::try_from(entry.oid).map_err(|_| fail_listing())?;
        let mode = Mode::from_bits(u32::from(entry.mode)).ok_or_else(fail_listing)?;
        if mode.is_sparse() {
            path.push(b'/');
        }
        children.push((path, id, mode));
        remaining = rest;
    }
    Ok(children)
}
fn fail_listing() -> DocumentError {
    DocumentError::new(
        DocumentErrorKind::InternalError,
        "cannot list document candidates",
    )
}
pub(super) fn parse_candidates(raw: &[u8]) -> DocumentResult<Vec<RepoPath>> {
    if !raw.is_empty() && !raw.ends_with(&[0]) {
        return Err(DocumentError::new(
            DocumentErrorKind::InternalError,
            "invalid Git candidate response",
        ));
    }
    let mut candidates = BTreeSet::new();
    for entry in raw
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        if !is_corpus_entry(entry) {
            continue;
        }
        let text = std::str::from_utf8(entry).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                "unsupported document path",
                cause,
            )
        })?;
        let path = RepoPath::parse(text).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                "unsupported document path",
                cause,
            )
        })?;
        candidates.insert(path);
    }
    Ok(candidates.into_iter().collect())
}

fn is_corpus_entry(entry: &[u8]) -> bool {
    (entry.ends_with(b".md") || entry.ends_with(b".markdown"))
        && !entry
            .split(|byte| *byte == b'/')
            .any(|part| part == b".git" || part == SERVICE_DIRECTORY.as_bytes())
}
