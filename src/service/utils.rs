use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, DiscoveryConfig, DocumentError,
    DocumentErrorKind, DocumentResult, FileReader, GitIgnoreMode, GitWorktreeListing,
    GraphDiagnostics, KnowledgeMetadata, NodeMetadata, PROVENANCE_BATCH_SIZE, ParsedFile,
    ProvenanceIssue, ProvenanceRequest, SourceProvenance, StorageRead,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri};
use crate::service::{FileErrorKind, FileResult};
use crate::utils::{CONFIG_PATH, IGNORE_PATH, SERVICE_DIRECTORY};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// Message reported when the document candidate set changed during a refresh.
pub(super) const DOCUMENTS_CHANGED_MESSAGE: &str = "documents changed during refresh";
pub(super) const CONFIG_READ_FAILURE: &str = "cannot read embedding configuration";
const CANDIDATE_LIST_MESSAGE: &str = "cannot list document candidates";
const FILE_OBSERVATION_WAIT: Duration = Duration::from_secs(5);
const FILE_OBSERVATION_RETRY: Duration = Duration::from_millis(1);
const UNSUPPORTED_DOCUMENT_PATH: &str = "unsupported document path";
pub(super) const PATH: &str = "path";
pub(super) const REASON: &str = "reason";
pub(super) const ELEMENT_ID: &str = "element_id";
pub(super) const TARGET: &str = "target";
const MISSING_SNAPSHOT: &str = "missing_source_snapshot";
const UNAVAILABLE_SNAPSHOT: &str = "source_snapshot_unavailable";
const SOURCE_NOT_FOUND: &str = "source_not_found";

/// Selects document candidates for one operation using borrowed infrastructure capabilities.
pub(super) struct CandidateSelection<'a> {
    pub(super) git: &'a dyn GitWorktreeListing,
    pub(super) filesystem: &'a dyn FileReader,
}

/// Bytes and presence observed for one corpus-selection policy.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DiscoveryObservation {
    config_bytes: Option<Vec<u8>>,
    ignore_bytes: Option<Vec<u8>>,
}

/// Selected paths and the policy that must still hold at publication.
#[derive(Debug)]
pub(super) struct CandidateObservation {
    pub(super) paths: Vec<RepoPath>,
    pub(super) discovery: DiscoveryObservation,
}

/// Borrows graph and provenance observers for one pinned check/status operation.
pub(super) struct CheckObservations<'a> {
    pub(super) storage_read: &'a dyn StorageRead,
    /// Graph-only diagnostics over the same supplied read view.
    pub(super) graph: &'a dyn GraphDiagnostics,
    /// Source observations with bounded batching for the selected owners.
    pub(super) provenance: &'a dyn SourceProvenance,
}

impl std::fmt::Debug for CandidateSelection<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CandidateSelection")
            .finish_non_exhaustive()
    }
}

impl CandidateSelection<'_> {
    /// Returns selected normalized document paths with the observed discovery files.
    ///
    /// Final symlinks and irrelevant raw names are filtered before decoding. Structurally
    /// unsafe raw paths skip leaf probes and keep `RepoPath` lexical validation.
    ///
    /// # Arguments
    ///
    /// * `paths` — Optional owner paths; empty selects the complete corpus.
    ///
    /// # Errors
    /// Reports document errors for Git, file observation, framing or path failures.
    pub(super) fn get_candidates(
        &self,
        paths: &[RepoPath],
    ) -> DocumentResult<CandidateObservation> {
        let discovery = self.get_discovery()?;
        let raw = self.get_raw_candidates(&discovery)?;
        let entries = get_candidate_entries(&raw)?;
        self.filesystem
            .directory_get_by_path(Path::new("."))
            .map_err(|cause| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    CANDIDATE_LIST_MESSAGE,
                    cause,
                )
            })?
            .ok_or_else(|| {
                DocumentError::new(DocumentErrorKind::InternalError, CANDIDATE_LIST_MESSAGE)
            })?;
        let mut filtered = Vec::new();
        for entry in entries {
            if !paths.is_empty()
                && !normalize_candidate_bytes(entry)
                    .is_some_and(|normalized| is_selected_path(&normalized, paths))
            {
                continue;
            }
            if is_safe_candidate_probe(entry)
                && self
                    .filesystem
                    .symlink_is_exist(Path::new(OsStr::from_bytes(entry)))
                    .map_err(|cause| {
                        DocumentError::new_with_cause(
                            DocumentErrorKind::InternalError,
                            CANDIDATE_LIST_MESSAGE,
                            cause,
                        )
                    })?
            {
                continue;
            }
            filtered.extend_from_slice(entry);
            filtered.push(0);
        }
        Ok(CandidateObservation {
            paths: parse_candidates(&filtered)?,
            discovery,
        })
    }

    /// Rejects a discovery policy edit made after the candidate observation.
    ///
    /// A same-stat replacement is also rejected.
    ///
    /// # Arguments
    ///
    /// * `previous` — Config and ignore bytes observed before selection or publication.
    ///
    /// # Errors
    /// Reports unreadable policy files or an observation that no longer matches.
    pub(super) fn verify_discovery(&self, previous: &DiscoveryObservation) -> DocumentResult<()> {
        if self.get_discovery()? != *previous {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
        }
        Ok(())
    }

    /// Returns raw Git candidate paths for this selection.
    ///
    /// The observed config and ignore bytes set the exclusion mode.
    ///
    /// # Arguments
    ///
    /// * `discovery` — Config and ignore bytes captured for this selection.
    ///
    /// # Errors
    /// Reports invalid discovery configuration or Git enumeration failures.
    fn get_raw_candidates(&self, discovery: &DiscoveryObservation) -> DocumentResult<Vec<u8>> {
        let configuration = discovery
            .config_bytes
            .as_deref()
            .map(DiscoveryConfig::parse)
            .transpose()
            .map_err(|cause| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InvalidRequest,
                    "invalid discovery configuration",
                    cause,
                )
            })?
            .unwrap_or_default();
        let mode = if configuration.is_gitignore_respected() {
            GitIgnoreMode::Standard
        } else {
            GitIgnoreMode::Disabled
        };
        self.git
            .worktree_paths_get_by_ignore_mode_and_patterns(
                mode,
                discovery.ignore_bytes.as_deref().unwrap_or_default(),
            )
            .map_err(|cause| {
                DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    CANDIDATE_LIST_MESSAGE,
                    cause,
                )
            })
    }

    /// Reads the config and ignore bytes.
    ///
    /// The two files define the current discovery policy.
    ///
    /// # Errors
    /// Reports unreadable or unsafe files; absence is not an error.
    fn get_discovery(&self) -> DocumentResult<DiscoveryObservation> {
        retry_changed_file_observation(|| {
            self.filesystem
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))?;
            Ok(DiscoveryObservation {
                config_bytes: self
                    .filesystem
                    .file_get_bytes_by_path(Path::new(CONFIG_PATH))?,
                ignore_bytes: self
                    .filesystem
                    .file_get_bytes_by_path(Path::new(IGNORE_PATH))?,
            })
        })
        .map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                CANDIDATE_LIST_MESSAGE,
                cause,
            )
        })
    }
}

/// Repeats a file observation that reports a transient change.
///
/// A presence flip between two observations is a transient `Changed` error.
/// A stable replacement or another failure is returned without retrying.
///
/// # Arguments
///
/// * `operation` — File observation retried while it reports a transient change.
///
/// # Errors
/// Reports the final file observation failure.
pub(crate) fn retry_changed_file_observation<T>(
    mut operation: impl FnMut() -> FileResult<T>,
) -> FileResult<T> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if error.get_kind() == FileErrorKind::Changed
                    && started.elapsed() < FILE_OBSERVATION_WAIT =>
            {
                thread::sleep(FILE_OBSERVATION_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Resolves raw dot components for scope matching.
///
/// Names are not decoded.
///
/// # Arguments
///
/// * `entry` — raw root-relative path bytes whose `.` and `..` components are resolved.
fn normalize_candidate_bytes(entry: &[u8]) -> Option<Vec<u8>> {
    if entry.starts_with(b"/") {
        return None;
    }
    let mut parts = Vec::new();
    for part in entry.split(|byte| *byte == b'/') {
        match part {
            b"" | b"." => {}
            b".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join(&b"/"[..]))
}

/// Returns whether the path is selected or lies under a selected path.
///
/// # Arguments
///
/// * `path` — Normalized root-relative path to test.
/// * `paths` — Owner selectors; empty selects every path.
pub(super) fn is_selected_path(path: &[u8], paths: &[RepoPath]) -> bool {
    paths.is_empty()
        || paths.iter().any(|selector| {
            let prefix = selector.as_str().as_bytes();
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with(b"/"))
        })
}

impl std::fmt::Debug for CheckObservations<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckObservations")
            .finish_non_exhaustive()
    }
}

impl CheckObservations<'_> {
    /// Collects sorted graph and source findings from one read view.
    ///
    /// # Arguments
    ///
    /// * `paths` — Owner paths to collect findings for; empty collects from the whole corpus.
    ///
    /// # Errors
    /// Reports graph diagnostics, storage or provenance failures without publishing changes.
    pub(super) fn collect_check_problems(
        &self,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let mut problems = self.graph.find_problems(paths)?;
        problems.extend(self.find_source_problems(paths)?);
        Ok(sort_problems(problems))
    }

    fn find_source_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
        let nodes = self.storage_read.nodes_get().map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot read corpus for check",
                cause,
            )
        })?;
        let sources = self.get_source_bindings()?;
        let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
        let empty = BTreeSet::new();
        let mut problems = Vec::new();
        let mut selected_nodes = nodes.iter().filter_map(|node| match &node.metadata {
            NodeMetadata::Knowledge(metadata)
                if selected.is_empty() || selected.contains(get_owner_path(&metadata.id)) =>
            {
                Some(metadata)
            }
            _ => None,
        });
        loop {
            let batch: Vec<_> = selected_nodes
                .by_ref()
                .take(PROVENANCE_BATCH_SIZE)
                .collect();
            if batch.is_empty() {
                break;
            }
            let requests: Vec<_> = batch
                .iter()
                .map(|metadata| ProvenanceRequest {
                    source_snapshot: metadata.source_snapshot.as_ref(),
                    sources: sources.get(&metadata.id).unwrap_or(&empty),
                })
                .collect();
            let observations =
                self.provenance
                    .observe_sources_batch(&requests)
                    .map_err(|cause| {
                        CommandError::new_with_cause(
                            CommandErrorKind::InternalError,
                            "cannot observe knowledge sources",
                            cause,
                        )
                    })?;
            for (metadata, observation) in batch.into_iter().zip(observations) {
                problems.extend(
                    observation
                        .issues
                        .iter()
                        .map(|issue| format_source_problem(metadata, issue)),
                );
            }
        }
        Ok(problems)
    }

    fn get_source_bindings(&self) -> CommandResult<BTreeMap<KnowledgeId, BTreeSet<SourceUri>>> {
        let mut sources: BTreeMap<KnowledgeId, BTreeSet<SourceUri>> = BTreeMap::new();
        for edge in self
            .storage_read
            .edges_get_by_source_id(None)
            .map_err(|cause| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot read corpus for check",
                    cause,
                )
            })?
        {
            if let EdgeTarget::Source(source) = edge.get_target() {
                sources
                    .entry(edge.get_source().clone())
                    .or_default()
                    .insert(source.clone());
            }
        }
        Ok(sources)
    }
}

/// Returns whether the pinned knowledge matches the parsed file.
///
/// Returns false when no parsed file is available or its element differs.
///
/// # Arguments
///
/// * `node` — Pinned knowledge metadata to compare against.
/// * `parsed` — Current parsed projection of the owning file, if available.
pub(super) fn is_current_knowledge(node: &KnowledgeMetadata, parsed: Option<&ParsedFile>) -> bool {
    parsed.is_some_and(|parsed| {
        parsed.elements.iter().any(|current| {
            current.element.id == node.id
                && current.element.hash == node.hash
                && current.element.lines == node.lines
        })
    })
}

/// Sorts findings by path, reason, element ID and target.
///
/// Duplicates are removed.
///
/// # Arguments
///
/// * `problems` — Findings to sort and deduplicate.
pub(super) fn sort_problems(mut problems: Vec<CommandProblem>) -> Vec<CommandProblem> {
    problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
    problems.dedup_by(|left, right| get_sort_key(left) == get_sort_key(right));
    problems
}

fn format_source_problem(metadata: &KnowledgeMetadata, issue: &ProvenanceIssue) -> CommandProblem {
    let (reason, message, target) = match issue {
        ProvenanceIssue::MissingSnapshot => (
            MISSING_SNAPSHOT,
            "knowledge source snapshot is missing",
            None,
        ),
        ProvenanceIssue::SnapshotUnavailable { snapshot } => (
            UNAVAILABLE_SNAPSHOT,
            "knowledge source snapshot is unavailable",
            Some(snapshot.as_str()),
        ),
        ProvenanceIssue::SourceNotFound { source } => (
            SOURCE_NOT_FOUND,
            "knowledge source is not found",
            Some(source.as_str()),
        ),
    };
    let mut details = BTreeMap::from([
        (PATH.into(), Value::from(get_owner_path(&metadata.id))),
        (REASON.into(), Value::from(reason)),
        (ELEMENT_ID.into(), Value::from(metadata.id.as_str())),
    ]);
    if let Some(target) = target {
        details.insert(TARGET.into(), Value::from(target));
    }
    CommandProblem {
        code: CommandErrorKind::ValidationFailed,
        message,
        details,
    }
}

pub(super) fn get_owner_path(id: &KnowledgeId) -> &str {
    id.as_str()
        .split_once('#')
        .map_or(id.as_str(), |(path, _)| path)
}

pub(super) fn get_detail<'a>(problem: &'a CommandProblem, key: &str) -> &'a str {
    problem
        .details
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
}

pub(super) fn get_sort_key(problem: &CommandProblem) -> [&str; 4] {
    [PATH, REASON, ELEMENT_ID, TARGET].map(|key| get_detail(problem, key))
}

/// Converts a document error to a command error.
///
/// # Arguments
///
/// * `cause` — Document failure to convert.
/// * `message` — Command-level message describing the operation.
pub(super) fn map_document_error(cause: DocumentError, message: &'static str) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, message, cause)
}

/// Returns `None` for a stable nonregular final entry.
///
/// Other file errors become document errors.
///
/// # Arguments
///
/// * `result` — File observation to convert.
pub(super) fn map_document_file<T>(result: FileResult<Option<T>>) -> DocumentResult<Option<T>> {
    match result {
        Err(error) if error.get_kind() == FileErrorKind::NonRegular => Ok(None),
        result => result.map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "document path changed during observation",
                cause,
            )
        }),
    }
}

/// Returns the decoded, normalized and deduplicated candidate paths.
///
/// # Arguments
///
/// * `raw` — NUL-terminated raw candidate paths to decode and normalize.
pub(super) fn parse_candidates(raw: &[u8]) -> DocumentResult<Vec<RepoPath>> {
    let mut candidates = BTreeSet::new();
    for entry in get_candidate_entries(raw)? {
        let text = std::str::from_utf8(entry).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                UNSUPPORTED_DOCUMENT_PATH,
                cause,
            )
        })?;
        let path = RepoPath::parse(text).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InvalidRequest,
                UNSUPPORTED_DOCUMENT_PATH,
                cause,
            )
        })?;
        candidates.insert(path);
    }
    Ok(candidates.into_iter().collect())
}

/// Returns whether a raw path is safe to probe before normalization.
///
/// # Arguments
///
/// * `entry` — Raw path whose components are checked for safe probing.
fn is_safe_candidate_probe(entry: &[u8]) -> bool {
    entry
        .split(|byte| *byte == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

/// Returns the raw candidate entries after checking NUL framing.
///
/// Entries are filtered without being decoded.
///
/// # Arguments
///
/// * `raw` — NUL-framed raw Git response to validate and split.
fn get_candidate_entries(raw: &[u8]) -> DocumentResult<Vec<&[u8]>> {
    if !raw.is_empty() && !raw.ends_with(&[0]) {
        return Err(DocumentError::new(
            DocumentErrorKind::InternalError,
            "invalid Git candidate response",
        ));
    }
    Ok(raw
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty() && is_corpus_entry(entry))
        .collect())
}

fn is_corpus_entry(entry: &[u8]) -> bool {
    (entry.ends_with(b".md") || entry.ends_with(b".markdown"))
        && !entry
            .split(|byte| *byte == b'/')
            .any(|part| part == b".git" || part == crate::utils::SERVICE_DIRECTORY.as_bytes())
}

#[cfg(test)]
#[path = "utils_test.rs"]
mod tests;
