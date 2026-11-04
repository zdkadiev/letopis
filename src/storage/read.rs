use super::{
    connection::map_sqlite,
    edges::get_edges_by_source_id,
    files::{get_file_by_path, get_files},
    fts::get_text_matches_by_expression,
    graph::{get_node_by_id, get_nodes},
    neighbors::{get_neighbor_page, get_node_ref_by_id, get_source_snapshots},
    query::{Project, build_query},
    status_counts::get_embedding_status,
    vectors::{
        VectorSchema, get_config_identity, get_failure_by_search_hash, get_nearest_vectors,
        get_query_error, get_vector_by_search_hash, get_vector_dimensions,
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
    /// Begin a read pinned to one cache revision and vector identity.
    ///
    /// # Errors
    /// Reports absent publication, malformed metadata or database failures.
    pub(super) fn begin_tx(connection: &'a mut Connection) -> StorageResult<Self> {
        Self::begin_available_tx(connection)?.ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "storage has no published revision",
            )
        })
    }

    /// Begin a pinned read, returning none when no nonzero revision is published.
    ///
    /// # Errors
    /// Reports malformed metadata or database failures.
    pub(super) fn begin_optional_tx(connection: &'a mut Connection) -> StorageResult<Option<Self>> {
        Ok(Self::begin_available_tx(connection)?.filter(|read| read.revision != 0))
    }

    // A project row may exist before the first nonzero publication revision.
    fn begin_available_tx(connection: &'a mut Connection) -> StorageResult<Option<Self>> {
        let transaction = connection.transaction().map_err(map_sqlite)?;
        let Some(revision) = Self::get_revision(&transaction)? else {
            return Ok(None);
        };
        let config_identity = get_config_identity(&transaction, VectorSchema::Attached)?;
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
    fn get_node_ref_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        get_node_ref_by_id(&self.transaction, id)
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
        get_vector_dimensions(&self.transaction, VectorSchema::Attached).map(VectorOutcome::Current)
    }
    fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        get_node_by_id(&self.transaction, id)
    }
    fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        get_source_snapshots(&self.transaction)
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
    fn get_text_matches_by_expression(
        &mut self,
        expression: &str,
    ) -> StorageResult<Vec<TextMatch>> {
        get_text_matches_by_expression(&self.transaction, expression)
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
    fn get_nearest_vectors(
        &mut self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        get_nearest_vectors(&self.transaction, query, dimensions).map(VectorOutcome::Current)
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
    fn get_revision(&self) -> u64 {
        self.revision
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.config_identity
    }
}
