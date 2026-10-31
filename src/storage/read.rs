use super::{
    connection::map_sqlite,
    files::{get_file_by_path, get_files},
    fts::match_text,
    graph::{get_edges_by_source_id, get_node_by_id, get_nodes},
    neighbors::{get_neighbor_page, get_source_snapshots, resolve_node_by_id},
    query::{Project, build_query},
    status_counts::get_embedding_status,
    vectors::{
        find_nearest_vectors, get_config_identity, get_failure_by_search_hash, get_query_error,
        get_vector_by_search_hash, get_vector_dimensions,
    },
    work::get_next_embedding_work_by_id_and_max_attempts,
};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        ElementAddress, EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary,
        EmbeddingWork, FileRow, GraphNeighbor, GraphNodeRef, GraphRead, NeighborPageRequest,
        QueryError, StorageError, StorageErrorKind, StorageRead, StorageResult, StoredNode,
        TextMatch, VectorMatch, VectorOutcome,
    },
};
use rusqlite::{Connection, OptionalExtension, Transaction, params_from_iter};
use sea_query::{Expr, ExprTrait, Query};
use std::{collections::BTreeSet, num::NonZeroUsize};

#[derive(Debug)]
pub(crate) struct SqliteRead<'a> {
    transaction: Transaction<'a>,
    revision: u64,
    config_identity: Option<Sha256>,
}
impl<'a> SqliteRead<'a> {
    pub(super) fn begin(connection: &'a mut Connection) -> StorageResult<Self> {
        Self::begin_available(connection)?.ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "storage has no published revision",
            )
        })
    }

    pub(super) fn begin_optional(connection: &'a mut Connection) -> StorageResult<Option<Self>> {
        Ok(Self::begin_available(connection)?.filter(|read| read.revision != 0))
    }

    fn begin_available(connection: &'a mut Connection) -> StorageResult<Option<Self>> {
        let transaction = connection.transaction().map_err(map_sqlite)?;
        let Some(revision) = Self::get_revision(&transaction)? else {
            return Ok(None);
        };
        let config_identity = get_config_identity(&transaction, true)?;
        Ok(Some(Self {
            transaction,
            revision,
            config_identity,
        }))
    }

    fn get_revision(connection: &Connection) -> StorageResult<Option<u64>> {
        let (sql, values) = build_query(
            Query::select()
                .column(Project::Revision)
                .from(Project::Table)
                .and_where(Expr::col(Project::Id).eq(1)),
        )?;
        let revision: Option<i64> = connection
            .query_row(&sql, params_from_iter(values), |row| row.get(0))
            .optional()
            .map_err(map_sqlite)?;
        revision.map(u64::try_from).transpose().map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid storage revision",
                error,
            )
        })
    }
}
impl GraphRead for SqliteRead<'_> {
    fn resolve_node_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        resolve_node_by_id(&self.transaction, id)
    }
    fn get_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        get_neighbor_page(&self.transaction, request)
    }
}
impl StorageRead for SqliteRead<'_> {
    fn get_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        get_embedding_status(&self.transaction, self.config_identity, policy)
    }
    fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        get_source_snapshots(&self.transaction)
    }
    fn get_next_embedding_work_by_id_and_max_attempts(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        id: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        get_next_embedding_work_by_id_and_max_attempts(&self.transaction, max_attempts, id)
            .map(VectorOutcome::Current)
    }
    fn get_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        get_vector_dimensions(&self.transaction, true).map(VectorOutcome::Current)
    }
    fn get_failure_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        get_failure_by_search_hash(&self.transaction, search_hash).map(VectorOutcome::Current)
    }
    fn get_query_error(&mut self) -> StorageResult<Option<QueryError>> {
        get_query_error(&self.transaction)
    }
    fn get_vector_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        get_vector_by_search_hash(&self.transaction, search_hash).map(VectorOutcome::Current)
    }
    fn find_nearest_vectors(
        &mut self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        find_nearest_vectors(&self.transaction, query, dimensions).map(VectorOutcome::Current)
    }
    fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        get_node_by_id(&self.transaction, id)
    }
    fn get_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        get_nodes(&self.transaction)
    }
    fn get_edges_by_source_id(
        &mut self,
        source_id: Option<&KnowledgeId>,
    ) -> StorageResult<Vec<Edge>> {
        get_edges_by_source_id(&self.transaction, source_id)
    }
    fn get_file_by_path(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        get_file_by_path(&self.transaction, path)
    }
    fn get_files(&mut self) -> StorageResult<Vec<FileRow>> {
        get_files(&self.transaction)
    }
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>> {
        match_text(&self.transaction, expression)
    }
    fn get_revision(&self) -> u64 {
        self.revision
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.config_identity
    }
}
