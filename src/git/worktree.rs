use super::ignore::paths_filter_by_patterns;
use super::{GitRepository, objects::tree_get_by_oid, tree::tree_entry_get};
use crate::service::{GitError, GitIgnoreMode, GitResult, SnapshotErrorKind};
use gix::glob::pattern::Case;
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
use std::{borrow::Cow, path::Path};

/// Enumerate raw index and selected untracked paths with caller-supplied Git ignore rules.
pub(super) fn worktree_paths_get(
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

fn index_paths_get(index: &State) -> Vec<u8> {
    let mut raw = Vec::new();
    for entry in index.entries() {
        raw.extend_from_slice(entry.path(index));
        raw.push(0);
    }
    raw
}

/// Expand only metadata in a private copy: excludes and walk need the same tracked view.
fn index_expand_sparse<'a>(git: &GitRepository, index: &'a State) -> GitResult<Cow<'a, State>> {
    if !index.is_sparse() {
        return Ok(Cow::Borrowed(index));
    }
    git.objects_require_self_contained()
        .map_err(GitError::new)?;
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
/// Keep only child metadata between reads; verified tree payloads never accumulate.
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
