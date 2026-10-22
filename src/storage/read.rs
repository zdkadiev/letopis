use super::{
    connection::map_sqlite,
    query::{Meta, Project, Schemas, build_query},
};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256},
    service::{
        EmbeddingFailure, FileRow, QueryError, StorageError, StorageErrorKind, StorageRead,
        StorageResult, StoredNode, TextMatch, VectorMatch, VectorOutcome,
    },
};
use rusqlite::{Connection, OptionalExtension, Transaction, params_from_iter};
use sea_query::{Expr, ExprTrait, Query};

pub struct SqliteRead<'a> {
    transaction: Transaction<'a>,
    revision: u64,
    config_identity: Option<Sha256>,
}
impl<'a> SqliteRead<'a> {
    pub(super) fn begin(connection: &'a mut Connection) -> StorageResult<Self> {
        let transaction = connection.transaction().map_err(map_sqlite)?;
        let (sql, values) = build_query(
            Query::select()
                .column(Project::Revision)
                .from(Project::Table)
                .and_where(Expr::col(Project::Id).eq(1)),
        )?;
        let revision: i64 = transaction
            .query_row(&sql, params_from_iter(values), |row| row.get(0))
            .map_err(map_sqlite)?;
        let revision = u64::try_from(revision).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid storage revision",
                error,
            )
        })?;
        let (sql, values) = build_query(
            Query::select()
                .column(Meta::ConfigIdentity)
                .from((Schemas::Vec, Meta::Table))
                .and_where(Expr::col(Meta::Id).eq(1)),
        )?;
        let identity: Option<String> = transaction
            .query_row(&sql, params_from_iter(values), |row| row.get(0))
            .optional()
            .map_err(map_sqlite)?;
        let config_identity = identity
            .map(|raw| Sha256::parse(&raw))
            .transpose()
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid stored configuration identity",
                    error,
                )
            })?;
        Ok(Self {
            transaction,
            revision,
            config_identity,
        })
    }
}
impl StorageRead for SqliteRead<'_> {
    fn read_failure(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        super::vectors::read_failure(&self.transaction, hash).map(VectorOutcome::Current)
    }
    fn read_query_error(&mut self) -> StorageResult<Option<QueryError>> {
        super::vectors::read_query_error(&self.transaction)
    }
    fn read_vector(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        if self.config_identity != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        super::vectors::read_vector(&self.transaction, hash).map(VectorOutcome::Current)
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
        super::vectors::find_nearest_vectors(&self.transaction, query, dimensions)
            .map(VectorOutcome::Current)
    }
    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        super::graph::read_node(&self.transaction, id)
    }
    fn list_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        super::graph::list_nodes(&self.transaction)
    }
    fn list_edges(&mut self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        super::graph::list_edges(&self.transaction, source)
    }
    fn read_file(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        super::files::read_file(&self.transaction, path)
    }
    fn list_files(&mut self) -> StorageResult<Vec<FileRow>> {
        super::files::list_files(&self.transaction)
    }
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>> {
        super::fts::match_text(&self.transaction, expression)
    }
    fn get_revision(&self) -> u64 {
        self.revision
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.config_identity
    }
}
