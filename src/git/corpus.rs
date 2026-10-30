use crate::{
    domain::RepoPath,
    service::{DocumentError, DocumentErrorKind, DocumentResult},
    utils::SERVICE_DIRECTORY,
};
use gix::Repository;
use gix_dir::{
    entry::{Kind, Status},
    walk::{Context, Options, delegate::Collect},
};
use gix_pathspec::Search;
use std::{collections::BTreeSet, path::Path};

/// List the actual worktree index plus non-ignored untracked files, without filters.
pub(super) fn list_candidates(repo: &Repository, root: &Path) -> DocumentResult<Vec<RepoPath>> {
    let index = repo.index_or_empty().map_err(|_| fail_listing())?;
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
    parse_candidates(&raw)
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
        if !entry.ends_with(b".md") && !entry.ends_with(b".markdown") {
            continue;
        }
        if entry
            .split(|byte| *byte == b'/')
            .any(|part| part == b".git" || part == SERVICE_DIRECTORY.as_bytes())
        {
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
