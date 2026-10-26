use super::{
    connection::map_sqlite,
    files::{list_files, read_file},
    fts::match_text,
    graph::{list_edges, list_nodes, read_node},
    neighbors::{list_source_snapshots, read_neighbor_page, resolve_node},
    query::{Meta, Project, Schemas, build_query},
    status_counts::read_embedding_status,
    vectors::{
        find_nearest_vectors, read_failure, read_query_error, read_vector, read_vector_dimensions,
    },
    work::read_next_embedding_work,
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
        let Some(revision) = Self::read_revision(&transaction)? else {
            return Ok(None);
        };
        let config_identity = Self::read_identity(&transaction)?;
        Ok(Some(Self {
            transaction,
            revision,
            config_identity,
        }))
    }

    fn read_revision(connection: &Connection) -> StorageResult<Option<u64>> {
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

    fn read_identity(connection: &Connection) -> StorageResult<Option<Sha256>> {
        let (sql, values) = build_query(
            Query::select()
                .column(Meta::ConfigIdentity)
                .from((Schemas::Vec, Meta::Table))
                .and_where(Expr::col(Meta::Id).eq(1)),
        )?;
        let identity: Option<String> = connection
            .query_row(&sql, params_from_iter(values), |row| row.get(0))
            .optional()
            .map_err(map_sqlite)?;
        identity
            .map(|raw| Sha256::parse(&raw))
            .transpose()
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid stored configuration identity",
                    error,
                )
            })
    }
}
impl GraphRead for SqliteRead<'_> {
    fn resolve_node(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        resolve_node(&self.transaction, id)
    }
    fn read_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        read_neighbor_page(&self.transaction, request)
    }
}
impl StorageRead for SqliteRead<'_> {
    fn read_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        read_embedding_status(&self.transaction, self.config_identity, policy)
    }
    fn list_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        list_source_snapshots(&self.transaction)
    }
    fn read_next_embedding_work(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        after: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        read_next_embedding_work(&self.transaction, max_attempts, after).map(VectorOutcome::Current)
    }
    fn read_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        read_vector_dimensions(&self.transaction, true).map(VectorOutcome::Current)
    }
    fn read_failure(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        read_failure(&self.transaction, hash).map(VectorOutcome::Current)
    }
    fn read_query_error(&mut self) -> StorageResult<Option<QueryError>> {
        read_query_error(&self.transaction)
    }
    fn read_vector(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        read_vector(&self.transaction, hash).map(VectorOutcome::Current)
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
    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        read_node(&self.transaction, id)
    }
    fn list_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        list_nodes(&self.transaction)
    }
    fn list_edges(&mut self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        list_edges(&self.transaction, source)
    }
    fn read_file(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        read_file(&self.transaction, path)
    }
    fn list_files(&mut self) -> StorageResult<Vec<FileRow>> {
        list_files(&self.transaction)
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
