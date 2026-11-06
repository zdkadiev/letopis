macro_rules! delegate_cache_reads { () => { delegate_cache_reads!(self => &self.inner); }; ($receiver:ident => $target:expr) => {

fn nodes_get_by_kind(&$receiver) -> $crate::service::StorageResult<u64> { $crate::service::StorageRead::nodes_get_by_kind($target) }

fn nodes_get_coverage_by_kind(&$receiver) -> $crate::service::StorageResult<u64> { $crate::service::StorageRead::nodes_get_coverage_by_kind($target) }

fn embed_failures_get_by_kind_and_search_hash_and_attempts(&$receiver, max_attempts: u64) -> $crate::service::StorageResult<Vec<$crate::service::EmbeddingFailure>> { $crate::service::StorageRead::embed_failures_get_by_kind_and_search_hash_and_attempts($target, max_attempts) }

fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
    &$receiver,
    expected: &$crate::domain::Sha256,
    max_attempts: u64,
    uri: Option<&$crate::domain::KnowledgeId>,
) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<$crate::service::EmbeddingWork>>> { $crate::service::StorageRead::nodes_get_by_kind_and_search_hash_and_attempts_and_uri($target, expected, max_attempts, uri) }

fn vector_index_config_get_by_id(
    &$receiver,
    expected: &$crate::domain::Sha256,
) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<std::num::NonZeroUsize>>> { $crate::service::StorageRead::vector_index_config_get_by_id($target, expected) }

fn nodes_get_by_uri(&$receiver, uri: &$crate::domain::EdgeTarget) -> $crate::service::StorageResult<Option<$crate::service::StoredNode>> { $crate::service::StorageRead::nodes_get_by_uri($target, uri) }

fn nodes_get_by_source_snapshot(&$receiver) -> $crate::service::StorageResult<std::collections::BTreeSet<$crate::domain::TreeOid>> { $crate::service::StorageRead::nodes_get_by_source_snapshot($target) }

fn nodes_get(&$receiver) -> $crate::service::StorageResult<Vec<$crate::service::StoredNode>> { $crate::service::StorageRead::nodes_get($target) }

fn edges_get_by_source_id(&$receiver, source_id: Option<&$crate::domain::KnowledgeId>) -> $crate::service::StorageResult<Vec<$crate::domain::Edge>> { $crate::service::StorageRead::edges_get_by_source_id($target, source_id) }

fn files_get_by_path(&$receiver, path: &$crate::domain::RepoPath) -> $crate::service::StorageResult<Option<$crate::service::FileRow>> { $crate::service::StorageRead::files_get_by_path($target, path) }

fn files_get(&$receiver) -> $crate::service::StorageResult<Vec<$crate::service::FileRow>> { $crate::service::StorageRead::files_get($target) }

fn text_index_get_by_expression(&$receiver, expression: &str) -> $crate::service::StorageResult<Vec<$crate::service::TextMatch>> { $crate::service::StorageRead::text_index_get_by_expression($target, expression) }

fn vector_keys_get_by_search_hash(
    &$receiver,
    expected: &$crate::domain::Sha256,
    search_hash: &$crate::domain::Sha256,
) -> $crate::service::StorageResult<$crate::service::VectorOutcome<Option<Vec<f32>>>> { $crate::service::StorageRead::vector_keys_get_by_search_hash($target, expected, search_hash) }

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

fn nodes_get_by_uri(&$receiver, uri: &$crate::service::ElementAddress) -> $crate::service::StorageResult<Option<$crate::service::GraphNodeRef>> { $crate::service::GraphRead::nodes_get_by_uri($target, uri) }

fn edges_get_by_row_id_and_type(
    &$receiver,
    request: &$crate::service::NeighborPageRequest<'_>,
) -> $crate::service::StorageResult<Vec<$crate::service::GraphNeighbor>> { $crate::service::GraphRead::edges_get_by_row_id_and_type($target, request) }
}; }
pub(crate) use delegate_graph_reads;
macro_rules! delegate_cache_writes { () => { delegate_cache_writes!(self => &self.inner); }; ($receiver:ident => $target:expr) => {

fn verify_transaction_scope(&$receiver, requirement: $crate::service::TransactionRequirement) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::verify_transaction_scope($target, requirement) }

fn project_get_revision(&$receiver) -> $crate::service::StorageResult<u64> { $crate::service::CacheDatabase::project_get_revision($target) }

fn nodes_upsert_by_id(&$receiver, node: &$crate::service::NodeMetadata) -> $crate::service::StorageResult<i64> { $crate::service::CacheDatabase::nodes_upsert_by_id($target, node) }

fn edges_save_by_source_id_and_target_id_and_type(&$receiver, edge: &$crate::domain::Edge) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::edges_save_by_source_id_and_target_id_and_type($target, edge) }

fn edges_delete_by_source_id(&$receiver, source_id: &$crate::domain::KnowledgeId) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::edges_delete_by_source_id($target, source_id) }

fn delete_node(&$receiver, id: &$crate::domain::EdgeTarget) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::delete_node($target, id) }

fn replace_text(&$receiver, id: &$crate::domain::KnowledgeId, text: &str) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::replace_text($target, id, text) }

fn files_upsert_by_path(&$receiver, file: &$crate::service::FileRow) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::files_upsert_by_path($target, file) }

fn files_delete_by_path(&$receiver, path: &$crate::domain::RepoPath) -> $crate::service::StorageResult<()> { $crate::service::CacheDatabase::files_delete_by_path($target, path) }
}; }
pub(crate) use delegate_cache_writes;
