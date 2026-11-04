use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, DocumentError,
    DocumentErrorKind, DocumentResult, FileReader, GitWorktreePaths, GraphDiagnostics,
    KnowledgeMetadata, NodeMetadata, PROVENANCE_BATCH_SIZE, ParsedFile, ProvenanceIssue,
    ProvenanceRequest, SourceProvenance, StorageError, StorageRead,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri};
use crate::service::{FileError, FileErrorKind, FileResult};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::Path,
};

/// Public context shared by document candidate observations.
const CANDIDATE_LIST_MESSAGE: &str = "cannot list document candidates";
const UNSUPPORTED_DOCUMENT_PATH: &str = "unsupported document path";
const PATH: &str = "path";
const REASON: &str = "reason";
const ELEMENT_ID: &str = "element_id";
const TARGET: &str = "target";
const MISSING_SNAPSHOT: &str = "missing_source_snapshot";
const UNAVAILABLE_SNAPSHOT: &str = "source_snapshot_unavailable";
const SOURCE_NOT_FOUND: &str = "source_not_found";

/// Selects document candidates for one operation using borrowed infrastructure capabilities.
pub(super) struct CandidateSelection<'a> {
    pub(super) git: &'a dyn GitWorktreePaths,
    pub(super) filesystem: &'a dyn FileReader,
}

/// Borrows graph and provenance observers for one pinned check/status operation.
pub(super) struct CheckObservations<'a> {
    pub(super) storage: &'a dyn StorageRead,
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
    /// Selects normalized document paths after root and no-follow symlink observations.
    /// Filters irrelevant raw names and final symlinks before decoding. Structurally
    /// unsafe raw paths skip leaf probes and retain `RepoPath` lexical validation.
    ///
    /// # Errors
    /// Preserves document diagnostics for Git/file observation, framing or path failures.
    pub(super) fn get_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        let raw = self.git.worktree_paths_get().map_err(map_candidate_error)?;
        let entries = get_candidate_entries(&raw)?;
        self.filesystem
            .directory_get_by_path(Path::new("."))
            .map_err(map_candidate_error)?
            .ok_or_else(|| {
                DocumentError::new(DocumentErrorKind::InternalError, CANDIDATE_LIST_MESSAGE)
            })?;
        let mut filtered = Vec::new();
        for entry in entries {
            if is_safe_candidate_probe(entry)
                && self
                    .filesystem
                    .symlink_is_exist(Path::new(OsStr::from_bytes(entry)))
                    .map_err(map_candidate_error)?
            {
                continue;
            }
            filtered.extend_from_slice(entry);
            filtered.push(0);
        }
        parse_candidates(&filtered)
    }
}

impl std::fmt::Debug for CheckObservations<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckObservations")
            .finish_non_exhaustive()
    }
}

impl CheckObservations<'_> {
    /// Collects sorted graph and source-provenance findings from one supplied read view.
    ///
    /// # Errors
    /// Returns graph diagnostics, storage or provenance failures without publishing changes.
    pub(super) fn collect_check_problems(
        &self,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let mut problems = self.graph.find_problems(paths)?;
        problems.extend(self.find_source_problems(paths)?);
        Ok(sort_problems(problems))
    }

    fn find_source_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
        let nodes = self.storage.nodes_get().map_err(map_storage_error)?;
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
            let observations = self
                .provenance
                .observe_sources_batch(&requests)
                .map_err(map_provenance_error)?;
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
            .storage
            .edges_get_by_source_id(None)
            .map_err(map_storage_error)?
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

/// Compares parsed knowledge data with the selected pinned metadata.
/// Returns false when no parsed knowledge file is available or its element differs.
pub(super) fn is_current_knowledge(node: &KnowledgeMetadata, parsed: Option<&ParsedFile>) -> bool {
    parsed.is_some_and(|parsed| {
        parsed.elements.iter().any(|current| {
            current.element.id == node.id
                && current.element.hash == node.hash
                && current.element.lines == node.lines
        })
    })
}

/// Sorts findings by path, reason, element ID and target, removing duplicates.
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

fn get_owner_path(id: &KnowledgeId) -> &str {
    id.as_str()
        .split_once('#')
        .map_or(id.as_str(), |(path, _)| path)
}

fn get_sort_key(problem: &CommandProblem) -> [&str; 4] {
    [PATH, REASON, ELEMENT_ID, TARGET].map(|key| {
        problem
            .details
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
    })
}

fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read corpus for check",
        cause,
    )
}
fn map_provenance_error(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot observe knowledge sources",
        cause,
    )
}

/// Exclude only a verified stable nonregular final document entry.
pub(super) fn map_document_file<T>(result: FileResult<Option<T>>) -> DocumentResult<Option<T>> {
    match result {
        Err(error) if error.get_kind() == FileErrorKind::NonRegular => Ok(None),
        result => result.map_err(map_document_file_error),
    }
}

pub(crate) fn map_document_file_error(cause: FileError) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "document path changed during observation",
        cause,
    )
}

pub(super) fn map_document_lock_error(cause: FileError) -> DocumentError {
    let (kind, message) = match cause.get_kind() {
        FileErrorKind::Aliased => (
            DocumentErrorKind::InvalidRequest,
            "private coordination lock required",
        ),
        FileErrorKind::NonRegular => (
            DocumentErrorKind::InvalidRequest,
            "regular document file required",
        ),
        _ => (
            DocumentErrorKind::InternalError,
            "cannot acquire coordination lock",
        ),
    };
    DocumentError::new_with_cause(kind, message, cause)
}

/// Decode, normalize and deduplicate selected paths, retaining safe path-error causes.
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

/// Only structurally safe raw paths may be observed before lexical `RepoPath` normalization.
fn is_safe_candidate_probe(entry: &[u8]) -> bool {
    entry
        .split(|byte| *byte == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

/// Validate NUL framing and filter relevant raw names without decoding them.
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

/// Preserve an infrastructure failure behind the document listing diagnostic.
fn map_candidate_error(cause: impl std::error::Error + Send + Sync + 'static) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        CANDIDATE_LIST_MESSAGE,
        cause,
    )
}

#[cfg(test)]
pub(crate) mod cache_test_support {
    macro_rules! delegate_cache_reads { () => { delegate_cache_reads!(self => &self.inner); }; ($receiver:ident => $target:expr) => {
fn get_embedding_status(
        &$receiver,
        policy: &$crate::service::EmbeddingStatusPolicy,
    ) -> $crate::service::StorageResult<$crate::service::EmbeddingStatusSummary> { $crate::service::StorageRead::get_embedding_status($target, policy) }
fn nodes_get_by_kind_and_search_hash_and_attempts_and_id(
        &$receiver,
        expected: &$crate::domain::Sha256,
        max_attempts: u64,
        id: Option<&$crate::domain::KnowledgeId>,
    ) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<$crate::service::EmbeddingWork>>> { $crate::service::StorageRead::nodes_get_by_kind_and_search_hash_and_attempts_and_id($target, expected, max_attempts, id) }
fn vector_index_config_get_by_id(
        &$receiver,
        expected: &$crate::domain::Sha256,
    ) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<std::num::NonZeroUsize>>> { $crate::service::StorageRead::vector_index_config_get_by_id($target, expected) }
fn nodes_get_by_id(&$receiver, id: &$crate::domain::EdgeTarget) -> $crate::service::StorageResult<Option<$crate::service::StoredNode>> { $crate::service::StorageRead::nodes_get_by_id($target, id) }
fn nodes_get_by_source_snapshot(&$receiver) -> $crate::service::StorageResult<std::collections::BTreeSet<$crate::domain::TreeOid>> { $crate::service::StorageRead::nodes_get_by_source_snapshot($target) }
fn nodes_get(&$receiver) -> $crate::service::StorageResult<Vec<$crate::service::StoredNode>> { $crate::service::StorageRead::nodes_get($target) }
fn edges_get_by_source_id(&$receiver, source_id: Option<&$crate::domain::KnowledgeId>) -> $crate::service::StorageResult<Vec<$crate::domain::Edge>> { $crate::service::StorageRead::edges_get_by_source_id($target, source_id) }
fn files_get_by_path(&$receiver, path: &$crate::domain::RepoPath) -> $crate::service::StorageResult<Option<$crate::service::FileRow>> { $crate::service::StorageRead::files_get_by_path($target, path) }
fn files_get(&$receiver) -> $crate::service::StorageResult<Vec<$crate::service::FileRow>> { $crate::service::StorageRead::files_get($target) }
fn text_index_get_by_expression(&$receiver, expression: &str) -> $crate::service::StorageResult<Vec<$crate::service::TextMatch>> { $crate::service::StorageRead::text_index_get_by_expression($target, expression) }
fn vector_keys_get_vector_by_search_hash(
        &$receiver,
        expected: &$crate::domain::Sha256,
        search_hash: &$crate::domain::Sha256,
    ) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<Vec<f32>>>> { $crate::service::StorageRead::vector_keys_get_vector_by_search_hash($target, expected, search_hash) }
fn vector_index_get_by_vector(
        &$receiver,
        expected: &$crate::domain::Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Vec<$crate::service::VectorMatch>>> { $crate::service::StorageRead::vector_index_get_by_vector($target, expected, query, dimensions) }
fn embed_failures_get_by_search_hash(
        &$receiver,
        expected: &$crate::domain::Sha256,
        search_hash: &$crate::domain::Sha256,
    ) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<$crate::service::EmbeddingFailure>>> { $crate::service::StorageRead::embed_failures_get_by_search_hash($target, expected, search_hash) }
fn query_error_get_by_id(&$receiver) -> $crate::service::StorageResult<Option<$crate::service::QueryError>> { $crate::service::StorageRead::query_error_get_by_id($target) }
fn project_get_by_id(&$receiver) -> $crate::service::StorageResult<u64> { $crate::service::StorageRead::project_get_by_id($target) }
fn meta_get_by_id(&$receiver) -> $crate::service::StorageResult<Option<$crate::domain::Sha256>> { $crate::service::StorageRead::meta_get_by_id($target) }
}; }
    pub(crate) use delegate_cache_reads;
    macro_rules! delegate_graph_reads { () => { delegate_graph_reads!(self => &self.inner); }; ($receiver:ident => $target:expr) => {
fn nodes_get_ref_by_id(&$receiver, id: &$crate::service::ElementAddress) -> $crate::service::StorageResult<Option<$crate::service::GraphNodeRef>> { $crate::service::GraphRead::nodes_get_ref_by_id($target, id) }
fn edges_get_by_nodes_row_id_and_type(
        &$receiver,
        request: &$crate::service::NeighborPageRequest<'_>,
    ) -> $crate::service::StorageResult<Vec<$crate::service::GraphNeighbor>> { $crate::service::GraphRead::edges_get_by_nodes_row_id_and_type($target, request) }
}; }
    pub(crate) use delegate_graph_reads;
    macro_rules! delegate_cache_writes { () => { delegate_cache_writes!(self => &self.inner); }; ($receiver:ident => $target:expr) => {
fn verify_transaction_scope(&$receiver, requirement: $crate::service::TransactionRequirement) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::verify_transaction_scope($target, requirement) }
fn get_publication_revision(&$receiver) -> $crate::service::StorageResult<u64> { $crate::service::CacheDatabase::get_publication_revision($target) }
fn nodes_upsert_by_id(&$receiver, node: &$crate::service::NodeMetadata) -> $crate::service::StorageResult<i64> { $crate::service::CacheDatabase::nodes_upsert_by_id($target, node) }
fn edges_save_by_source_id_and_target_id_and_type(&$receiver, edge: &$crate::domain::Edge) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::edges_save_by_source_id_and_target_id_and_type($target, edge) }
fn edges_delete_by_source_id(&$receiver, source_id: &$crate::domain::KnowledgeId) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::edges_delete_by_source_id($target, source_id) }
fn delete_node(&$receiver, id: &$crate::domain::EdgeTarget) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::delete_node($target, id) }
fn replace_text(&$receiver, id: &$crate::domain::KnowledgeId, text: &str) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::replace_text($target, id, text) }
fn files_upsert_by_path(&$receiver, file: &$crate::service::FileRow) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::files_upsert_by_path($target, file) }
fn files_delete_by_path(&$receiver, path: &$crate::domain::RepoPath) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::files_delete_by_path($target, path) }
}; }
    pub(crate) use delegate_cache_writes;
}
