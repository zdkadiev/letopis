use super::{
    connection::STORAGE_FAILURE,
    query::{Edges, Files, Nodes, Project, TextIndex, UnresolvedEdges, build_query},
    transaction::{self, State},
    vector::{
        VectorSchema, decode_config_identity,
        embed_failures_get_by_kind_and_search_hash_and_attempts, embed_failures_get_by_search_hash,
        meta_get_by_id, nodes_get_by_kind, nodes_get_by_kind_and_search_hash_and_attempts_and_uri,
        nodes_get_coverage_by_kind, query_error_get_by_id, vector_index_config_get_by_id,
        vector_index_get_nearest_by_vector, vector_keys_get_by_search_hash,
    },
};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, INCLUDES_EDGE_TYPE, KnowledgeId, LineRange,
        RepoPath, Sha256, SourceUri, TreeOid, ValueError,
    },
    service::{
        CacheDatabase, ElementAddress, EmbeddingFailure, EmbeddingWork, FileRow, GraphNeighbor,
        GraphNodeKey, GraphNodeRef, GraphRead, KnowledgeMetadata, NEIGHBOR_PAGE_QUANTUM,
        NeighborDirection, NeighborPageRequest, NodeMetadata, QueryError, SourceMetadata,
        StorageError, StorageErrorKind, StorageRead, StorageResult, StoredNode, TextMatch,
        TransactionError, TransactionRequirement, TraversalMode, VectorMatch, VectorOutcome,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use sea_query::{
    Expr, ExprTrait, Iden, JoinType, OnConflict, Order, Query, SelectStatement, UnionType, Value,
};
use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize};

pub(super) const SOURCE_KIND: &str = "source";
const VERTEX_KIND: &str = "vertex";
const METAVERTEX_KIND: &str = "metavertex";
const NODE_COLUMNS: [Nodes; 8] = [
    Nodes::Uri,
    Nodes::Kind,
    Nodes::Hash,
    Nodes::CreatedAt,
    Nodes::SourceSnapshot,
    Nodes::LineStart,
    Nodes::LineEnd,
    Nodes::SearchHash,
];

const FILE_COLUMNS: [Files; 8] = [
    Files::Path,
    Files::Size,
    Files::MtimeNs,
    Files::Inode,
    Files::Hash,
    Files::CheckedAtNs,
    Files::Valid,
    Files::Error,
];

impl From<ValueError> for StorageError {
    fn from(error: ValueError) -> Self {
        Self::new_with_cause(
            StorageErrorKind::InternalError,
            "invalid persisted storage value",
            error,
        )
    }
}

#[derive(Clone, Copy, Iden)]
enum Endpoint {
    Source,
    Target,
}

#[derive(Clone, Copy, Iden)]
enum Declaration {
    SourceUri,
    TargetUri,
    Type,
}

/// Read-only and writable `SQLite` connections for the cache database.
#[derive(Debug)]
pub(crate) struct CacheStorage {
    // Close the read-only vector attachment before the combined connection's
    // writable attachment, so the last vector owner can remove WAL sidecars.
    pub(super) writer: Option<Connection>,
    pub(super) read: Option<Connection>,
    state: Cell<State>,
}

impl CacheStorage {
    pub(crate) fn new(read: Connection, writer: Connection) -> Self {
        Self {
            read: Some(read),
            writer: Some(writer),
            state: Cell::new(State::Idle),
        }
    }

    pub(crate) fn new_observed(read: Option<Connection>) -> Self {
        Self {
            read,
            writer: None,
            state: Cell::new(State::Idle),
        }
    }

    #[cfg(test)]
    pub(crate) fn get_test_writer(&self) -> &Connection {
        self.writer.as_ref().unwrap()
    }

    #[cfg(test)]
    pub(crate) fn get_test_reader(&self) -> &Connection {
        self.read.as_ref().unwrap()
    }

    fn get_scope_connection(&self) -> StorageResult<&Connection> {
        let connection = if self.state.get() == State::Writing {
            &self.writer
        } else {
            &self.read
        };
        connection.as_ref().ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache connection is unavailable",
            )
        })
    }

    fn get_connection(&self) -> StorageResult<&Connection> {
        match self.state.get() {
            State::Reading | State::Writing => self.get_scope_connection(),
            _ => Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache scope is not active",
            )),
        }
    }

    fn get_write_connection(&self) -> StorageResult<&Connection> {
        if self.state.get() != State::Writing {
            if self.state.get() == State::Reading {
                // A caught write failure must still invalidate the enclosing scope.
                self.state.set(State::Failed);
            }
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache write scope is not active",
            ));
        }
        self.get_scope_connection()
    }

    fn record<T>(&self, result: StorageResult<T>) -> StorageResult<T> {
        if result.is_err() {
            self.state.set(State::Failed);
        }
        result
    }

    fn advance_revision(&self) -> StorageResult<()> {
        resolve_edges(self.get_write_connection()?)?;
        let (sql, values) = build_query(
            Query::select()
                .column(Project::Revision)
                .from(Project::Table)
                .and_where(Expr::col(Project::Id).eq(1)),
        )?;
        let revision: i64 =
            self.get_write_connection()?
                .query_row(&sql, params_from_iter(values), |row| row.get(0))?;
        let next = revision
            .checked_add(1)
            .filter(|_| revision >= 0)
            .ok_or_else(|| {
                StorageError::new(StorageErrorKind::InternalError, "storage revision overflow")
            })?;
        let (sql, values) = build_query(
            Query::update()
                .table(Project::Table)
                .value(Project::Revision, next)
                .and_where(Expr::col(Project::Id).eq(1)),
        )?;
        self.get_write_connection()?
            .execute(&sql, params_from_iter(values))?;
        Ok(())
    }
}

impl CacheDatabase for CacheStorage {
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()> {
        transaction::verify_scope(&self.state, requirement)
    }

    fn with_status_read_tx<T, E, F>(&self, operation: F) -> Result<Option<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if self.read.is_none() && self.state.get() == State::Idle {
            return Ok(None);
        }
        let connection = self
            .get_scope_connection()
            .map_err(TransactionError::Storage)?;
        transaction::run_read(connection, &self.state, || {
            let (sql, values) = build_query(
                Query::select()
                    .column(Project::Revision)
                    .from(Project::Table)
                    .and_where(Expr::col(Project::Id).eq(1)),
            )
            .map_err(TransactionError::Storage)?;
            let revision: Option<i64> = connection
                .query_row(&sql, params_from_iter(values), |row| row.get(0))
                .optional()
                .map_err(StorageError::from)
                .map_err(TransactionError::Storage)?;
            if revision.is_none() || revision == Some(0) {
                return Ok(None);
            }
            self.project_get_by_id()
                .map_err(TransactionError::Storage)?;
            self.meta_get_by_id().map_err(TransactionError::Storage)?;
            operation().map(Some).map_err(TransactionError::Operation)
        })
        .map_err(TransactionError::flatten)
    }

    fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let connection = self
            .get_scope_connection()
            .map_err(TransactionError::Storage)?;
        transaction::run_read(connection, &self.state, || {
            self.project_get_by_id()
                .map_err(TransactionError::Storage)?;
            self.meta_get_by_id().map_err(TransactionError::Storage)?;
            operation().map_err(TransactionError::Operation)
        })
        .map_err(TransactionError::flatten)
    }

    fn project_get_revision(&self) -> StorageResult<u64> {
        if self.state.get() != State::Writing {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache write scope is not active",
            ));
        }
        self.project_get_by_id()?
            .checked_add(1)
            .filter(|revision| i64::try_from(*revision).is_ok())
            .ok_or_else(|| {
                StorageError::new(StorageErrorKind::InternalError, "storage revision overflow")
            })
    }

    fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let writer = self.writer.as_ref().ok_or_else(|| {
            if matches!(self.state.get(), State::Reading | State::Writing) {
                self.state.set(State::Failed);
            }
            TransactionError::Storage(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache writer is unavailable",
            ))
        })?;
        transaction::run(writer, &self.state, operation, || self.advance_revision())
    }

    fn nodes_upsert_by_id(&self, node: &NodeMetadata) -> StorageResult<i64> {
        let db = self.get_write_connection()?;
        let result = (|| {
            let query = Query::insert()
                .into_table(Nodes::Table)
                .columns(NODE_COLUMNS)
                .values(build_node_values(node)?.map(Expr::val))
                .map_err(|error| {
                    StorageError::new_with_cause(
                        StorageErrorKind::InternalError,
                        "invalid persisted storage value",
                        error,
                    )
                })?
                .on_conflict(
                    OnConflict::column(Nodes::Uri)
                        .update_columns(NODE_COLUMNS.into_iter().skip(1))
                        .to_owned(),
                )
                .returning_col(Nodes::Id)
                .to_owned();
            let (sql, values) = build_query(&query)?;
            db.query_row(&sql, params_from_iter(values), |row| row.get(0))
                .map_err(map_mutation)
        })();
        self.record(result)
    }

    fn edges_save_by_source_id_and_target_id_and_type(&self, edge: &Edge) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        self.record(insert_edge(db, edge))
    }

    fn edges_delete_by_source_id(&self, source_id: &KnowledgeId) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        self.record(delete_outgoing(db, source_id))
    }

    fn delete_node(&self, id: &EdgeTarget) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        self.record(delete_node(db, id))
    }

    fn replace_text(&self, id: &KnowledgeId, text: &str) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        let result = (|| {
            let (sql, values) = build_query(
                Query::select()
                    .column(Nodes::Id)
                    .from(Nodes::Table)
                    .and_where(Expr::col(Nodes::Uri).eq(id.as_str()))
                    .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND)),
            )?;
            let row: Option<i64> = db
                .query_row(&sql, params_from_iter(values), |row| row.get(0))
                .optional()?;
            let row = row.ok_or_else(|| {
                StorageError::new(StorageErrorKind::NotFound, "knowledge metadata is absent")
            })?;
            let (sql, values) = build_query(
                Query::delete()
                    .from_table(TextIndex::Table)
                    .and_where(Expr::col(TextIndex::Rowid).eq(row)),
            )?;
            db.execute(&sql, params_from_iter(values))
                .map_err(map_mutation)?;
            let (sql, values) = build_query(
                Query::insert()
                    .into_table(TextIndex::Table)
                    .columns([TextIndex::Rowid, TextIndex::Text])
                    .values([Expr::val(row), Expr::val(text)])
                    .map_err(|error| {
                        StorageError::new_with_cause(
                            StorageErrorKind::InternalError,
                            "invalid persisted storage value",
                            error,
                        )
                    })?,
            )?;
            db.execute(&sql, params_from_iter(values))
                .map_err(map_mutation)?;
            Ok(())
        })();
        self.record(result)
    }

    fn files_upsert_by_path(&self, file: &FileRow) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        let result = (|| {
            let values: [Value; 8] = [
                file.path.as_str().into(),
                to_signed(file.size)?.into(),
                file.mtime_ns.into(),
                to_signed(file.inode)?.into(),
                file.hash.to_hex().into(),
                file.checked_at_ns.into(),
                file.valid.into(),
                file.error.as_deref().into(),
            ];
            let (sql, values) = build_query(
                Query::insert()
                    .into_table(Files::Table)
                    .columns(FILE_COLUMNS)
                    .values(values.map(Expr::val))
                    .map_err(|error| {
                        StorageError::new_with_cause(
                            StorageErrorKind::InternalError,
                            "invalid persisted storage value",
                            error,
                        )
                    })?
                    .on_conflict(
                        OnConflict::column(Files::Path)
                            .update_columns(FILE_COLUMNS.into_iter().skip(1))
                            .to_owned(),
                    ),
            )?;
            db.execute(&sql, params_from_iter(values))
                .map_err(map_mutation)?;
            Ok(())
        })();
        self.record(result)
    }

    fn files_delete_by_path(&self, path: &RepoPath) -> StorageResult<()> {
        let db = self.get_write_connection()?;
        let result = (|| {
            let (sql, values) = build_query(
                Query::delete()
                    .from_table(Files::Table)
                    .and_where(Expr::col(Files::Path).eq(path.as_str())),
            )?;
            db.execute(&sql, params_from_iter(values))
                .map_err(map_mutation)?;
            Ok(())
        })();
        self.record(result)
    }
}

impl StorageRead for CacheStorage {
    fn nodes_get_by_kind(&self) -> StorageResult<u64> {
        nodes_get_by_kind(self.get_connection()?)
    }

    fn nodes_get_coverage_by_kind(&self) -> StorageResult<u64> {
        nodes_get_coverage_by_kind(self.get_connection()?)
    }

    fn embed_failures_get_by_kind_and_search_hash_and_attempts(
        &self,
        max_attempts: u64,
    ) -> StorageResult<Vec<EmbeddingFailure>> {
        embed_failures_get_by_kind_and_search_hash_and_attempts(
            self.get_connection()?,
            max_attempts,
        )
    }

    fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
        &self,
        expected: &Sha256,
        max_attempts: u64,
        uri: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        if self.meta_get_by_id()? != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
            self.get_connection()?,
            max_attempts,
            uri,
        )
        .map(VectorOutcome::Current)
    }

    fn vector_index_config_get_by_id(
        &self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        if self.meta_get_by_id()? != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        vector_index_config_get_by_id(self.get_connection()?, VectorSchema::Attached)
            .map(VectorOutcome::Current)
    }

    fn nodes_get_by_uri(&self, uri: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .column(Nodes::Id)
                .columns(NODE_COLUMNS)
                .from(Nodes::Table)
                .and_where(Expr::col(Nodes::Uri).eq(uri.as_str())),
        )?;
        db.query_row(&sql, params_from_iter(values), |row| Ok(decode_node(row)))
            .optional()?
            .transpose()
    }

    fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(&nodes_get_by_source_snapshot())?;
        let mut statement = db.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut snapshots = BTreeSet::new();
        while let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            snapshots.insert(TreeOid::parse(&raw).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?);
        }
        Ok(snapshots)
    }

    fn nodes_get(&self) -> StorageResult<Vec<StoredNode>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .column(Nodes::Id)
                .columns(NODE_COLUMNS)
                .from(Nodes::Table)
                .order_by(Nodes::Uri, Order::Asc),
        )?;
        let mut statement = db.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(decode_node(row)?);
        }
        Ok(nodes)
    }

    fn edges_get_by_source_id(&self, source_id: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        edges_get_by_source_id(self.get_connection()?, source_id)
    }

    fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .columns(FILE_COLUMNS)
                .from(Files::Table)
                .and_where(Expr::col(Files::Path).eq(path.as_str())),
        )?;
        db.query_row(&sql, params_from_iter(values), |row| Ok(decode_file(row)))
            .optional()?
            .transpose()
    }

    fn files_get(&self) -> StorageResult<Vec<FileRow>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .columns(FILE_COLUMNS)
                .from(Files::Table)
                .order_by(Files::Path, Order::Asc),
        )?;
        let mut statement = db.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut files = Vec::new();
        while let Some(row) = rows.next()? {
            files.push(decode_file(row)?);
        }
        Ok(files)
    }

    fn text_index_get_by_expression(
        &self,
        expression: &str,
        limit: NonZeroUsize,
    ) -> StorageResult<Vec<TextMatch>> {
        let db = self.get_connection()?;
        let mut stmt = db.prepare(
            "select nodes.uri, nodes.id, bm25(text_index) as score
                 from text_index
                 join nodes on nodes.id = text_index.rowid
                 where text_index match ?1 and nodes.kind <> ?2
                 order by score, nodes.uri
                 limit ?3",
        )?;
        let mut rows = stmt
            .query(params![
                expression,
                SOURCE_KIND,
                i64::try_from(limit.get()).map_err(|error| {
                    StorageError::new_with_cause(
                        StorageErrorKind::InternalError,
                        "invalid persisted storage value",
                        error,
                    )
                })?
            ])
            .map_err(map_match)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(map_match)? {
            let id: String = row.get(0)?;
            found.push(TextMatch {
                id: KnowledgeId::parse(&id).map_err(|error| {
                    StorageError::new_with_cause(
                        StorageErrorKind::InternalError,
                        "invalid persisted storage value",
                        error,
                    )
                })?,
                row_id: row.get(1)?,
                bm25: row.get(2)?,
            });
        }
        Ok(found)
    }

    fn vector_keys_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        if self.meta_get_by_id()? != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        vector_keys_get_by_search_hash(self.get_connection()?, search_hash)
            .map(VectorOutcome::Current)
    }

    fn vector_index_get_by_vector(
        &self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
        limit: NonZeroUsize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        if self.meta_get_by_id()? != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        vector_index_get_nearest_by_vector(self.get_connection()?, query, dimensions, limit)
            .map(VectorOutcome::Current)
    }

    fn embed_failures_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        if self.meta_get_by_id()? != Some(*expected) {
            return Ok(VectorOutcome::ConfigChanged);
        }
        embed_failures_get_by_search_hash(
            self.get_connection()?,
            search_hash,
            VectorSchema::Attached,
        )
        .map(VectorOutcome::Current)
    }

    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>> {
        query_error_get_by_id(self.get_connection()?, VectorSchema::Attached)
    }

    fn project_get_by_id(&self) -> StorageResult<u64> {
        let connection = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .column(Project::Revision)
                .from(Project::Table)
                .and_where(Expr::col(Project::Id).eq(1)),
        )?;
        let revision: i64 =
            connection.query_row(&sql, params_from_iter(values), |row| row.get(0))?;
        u64::try_from(revision).map_err(|cause| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid storage revision",
                cause,
            )
        })
    }

    fn meta_get_by_id(&self) -> StorageResult<Option<Sha256>> {
        decode_config_identity(meta_get_by_id(
            self.get_connection()?,
            VectorSchema::Attached,
        )?)
    }
}

impl GraphRead for CacheStorage {
    fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        let db = self.get_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .columns([Nodes::Id, Nodes::Uri, Nodes::Kind])
                .from(Nodes::Table)
                .and_where(Expr::col(Nodes::Uri).eq(uri.as_str())),
        )?;
        db.query_row(&sql, params_from_iter(values), |row| {
            Ok(decode_node_ref(row))
        })
        .optional()?
        .transpose()
    }

    fn edges_get_by_row_id_and_type(
        &self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        let db = self.get_connection()?;
        if request.limit.get() > NEIGHBOR_PAGE_QUANTUM {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid neighbor page limit",
            ));
        }
        let (sql, values) = build_query(&edges_get_by_row_id_and_type(request))?;
        let mut statement = db.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut neighbors = Vec::new();
        while let Some(row) = rows.next()? {
            neighbors.push(decode_neighbor(row)?);
        }
        Ok(neighbors)
    }
}

fn build_node_values(node: &NodeMetadata) -> StorageResult<[Value; 8]> {
    if matches!(node, NodeMetadata::Knowledge(n) if n.id != n.uri) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "knowledge identity must equal its address",
        ));
    }
    let (uri, kind, hash, created, snapshot, lines, search) = match node {
        NodeMetadata::Knowledge(n) => (
            n.uri.as_str(),
            match n.kind {
                ElementKind::Vertex => VERTEX_KIND,
                ElementKind::Metavertex => METAVERTEX_KIND,
            },
            Some(n.hash.to_hex()),
            n.created_at,
            n.source_snapshot.as_ref().map(TreeOid::as_str),
            Some(n.lines),
            Some(n.search_hash.to_hex()),
        ),
        NodeMetadata::Source(n) => (
            n.uri.as_str(),
            SOURCE_KIND,
            n.hash.map(|h| h.to_hex()),
            n.created_at,
            None,
            n.lines,
            None,
        ),
    };
    let start = lines.map(|r| to_signed(r.get_start())).transpose()?;
    let end = lines.map(|r| to_signed(r.get_end())).transpose()?;
    Ok([
        uri.into(),
        kind.into(),
        hash.into(),
        to_signed(created)?.into(),
        snapshot.into(),
        start.into(),
        end.into(),
        search.into(),
    ])
}

fn decode_node(row: &Row<'_>) -> StorageResult<StoredNode> {
    let uri: String = row.get(1)?;
    let kind: String = row.get(2)?;
    let metadata = if kind == SOURCE_KIND {
        NodeMetadata::Source(decode_source_metadata(row)?)
    } else {
        NodeMetadata::Knowledge(decode_knowledge_metadata(row, &uri, &kind)?)
    };
    Ok(StoredNode {
        row_id: row.get(0)?,
        metadata,
    })
}

fn decode_source_metadata(row: &Row<'_>) -> StorageResult<SourceMetadata> {
    let uri: String = row.get(1)?;
    let hash: Option<String> = row.get(3)?;
    let created_at = get_unsigned(row.get(4)?)?;
    let snapshot: Option<String> = row.get(5)?;
    let lines = decode_lines(row.get(6)?, row.get(7)?)?;
    let search: Option<String> = row.get(8)?;
    let hash = hash
        .as_deref()
        .map(Sha256::parse)
        .transpose()
        .map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?;
    if snapshot.is_some() || search.is_some() {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid persisted metadata",
        ));
    }
    Ok(SourceMetadata {
        uri: SourceUri::parse(&uri).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
        hash,
        created_at,
        lines,
    })
}

fn decode_knowledge_metadata(
    row: &Row<'_>,
    uri: &str,
    kind: &str,
) -> StorageResult<KnowledgeMetadata> {
    let hash: Option<String> = row.get(3)?;
    let created_at = get_unsigned(row.get(4)?)?;
    let snapshot: Option<String> = row.get(5)?;
    let lines = decode_lines(row.get(6)?, row.get(7)?)?;
    let search: Option<String> = row.get(8)?;
    let hash = hash.as_deref().map(Sha256::parse).transpose()?;
    let kind = decode_element_kind(kind)?;
    Ok(KnowledgeMetadata {
        id: KnowledgeId::parse(uri)?,
        uri: KnowledgeId::parse(uri)?,
        kind,
        hash: hash.ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "invalid persisted metadata",
            )
        })?,
        created_at,
        source_snapshot: snapshot.as_deref().map(TreeOid::parse).transpose()?,
        lines: lines.ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "invalid persisted metadata",
            )
        })?,
        search_hash: Sha256::parse(&search.ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "invalid persisted metadata",
            )
        })?)?,
    })
}

fn decode_element_kind(kind: &str) -> StorageResult<ElementKind> {
    match kind {
        VERTEX_KIND => Ok(ElementKind::Vertex),
        METAVERTEX_KIND => Ok(ElementKind::Metavertex),
        _ => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid persisted metadata",
        )),
    }
}

fn decode_lines(start: Option<i64>, end: Option<i64>) -> StorageResult<Option<LineRange>> {
    match (start, end) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) => Ok(Some(
            LineRange::new(get_unsigned(a)?, get_unsigned(b)?).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
        )),
        _ => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid persisted metadata",
        )),
    }
}

fn to_signed(integer: u64) -> StorageResult<i64> {
    i64::try_from(integer).map_err(|e| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "storage integer out of range",
            e,
        )
    })
}

fn decode_file(row: &Row<'_>) -> StorageResult<FileRow> {
    let path: String = row.get(0)?;
    let size = get_unsigned(row.get(1)?)?;
    let mtime_ns = row.get(2)?;
    let inode = get_unsigned(row.get(3)?)?;
    let hash: String = row.get(4)?;
    let checked_at_ns = row.get(5)?;
    let valid: i64 = row.get(6)?;
    let error: Option<String> = row.get(7)?;
    if ![0, 1].contains(&valid) || (valid == 1) != error.is_none() {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid persisted file state",
        ));
    }
    Ok(FileRow {
        path: RepoPath::parse(&path).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
        size,
        mtime_ns,
        inode,
        hash: Sha256::parse(&hash).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
        checked_at_ns,
        valid: valid == 1,
        error,
    })
}

fn get_unsigned(integer: i64) -> StorageResult<u64> {
    u64::try_from(integer).map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InternalError,
            "invalid persisted storage integer",
            error,
        )
    })
}

/// Inserts one logical declaration, retaining absent targets without placeholder nodes.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
/// * `edge` — the validated edge to persist, whose source must already exist.
///
/// # Errors
/// Rejects missing sources and duplicates in either table as invalid requests;
/// reports database failures. The caller owns the surrounding transaction.
fn insert_edge(db: &Connection, edge: &Edge) -> StorageResult<()> {
    let source_id = nodes_get_id_by_uri(db, edge.get_source().as_str())?.ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage declaration",
        )
    })?;
    let target_uri = edge.get_target().as_str();
    let target_id = nodes_get_id_by_uri(db, target_uri)?;
    verify_no_duplicate(db, source_id, target_uri, edge.get_edge_type().as_str())?;
    let mut query = Query::insert();
    if let Some(target_id) = target_id {
        query
            .into_table(Edges::Table)
            .columns([Edges::SourceId, Edges::TargetId, Edges::Type])
            .values([
                Expr::val(source_id),
                Expr::val(target_id),
                Expr::val(edge.get_edge_type().as_str()),
            ])
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?;
    } else {
        query
            .into_table(UnresolvedEdges::Table)
            .columns([
                UnresolvedEdges::SourceId,
                UnresolvedEdges::TargetUri,
                UnresolvedEdges::Type,
            ])
            .values([
                Expr::val(source_id),
                Expr::val(target_uri),
                Expr::val(edge.get_edge_type().as_str()),
            ])
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?;
    }
    let (sql, values) = build_query(&query)?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

/// Returns resolved and unresolved declarations ordered by source URI, target URI and type.
///
/// # Arguments
///
/// * `db` — the read connection holding an active scope.
/// * `source_id` — the optional source filter; `None` returns declarations from every source.
///
/// # Errors
/// Reports query failures or invalid persisted domain values.
fn edges_get_by_source_id(
    db: &Connection,
    source_id: Option<&KnowledgeId>,
) -> StorageResult<Vec<Edge>> {
    let mut resolved = build_resolved_query();
    let mut unresolved = build_unresolved_query();
    if let Some(source_id) = source_id {
        let filter = Expr::col((Endpoint::Source, Nodes::Uri)).eq(source_id.as_str());
        resolved.and_where(filter.clone());
        unresolved.and_where(filter);
    }
    resolved.union(UnionType::All, unresolved);
    for column in [
        Declaration::SourceUri,
        Declaration::TargetUri,
        Declaration::Type,
    ] {
        resolved.order_by(column, Order::Asc);
    }
    let (sql, values) = build_query(&resolved)?;
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut edges = Vec::new();
    while let Some(row) = rows.next()? {
        edges.push(decode_edge(row)?);
    }
    Ok(edges)
}

/// Removes both declaration representations owned by a source; unknown owners are a no-op.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
/// * `source` — the source whose resolved and unresolved declarations are removed.
///
/// # Errors
/// Reports query or database failures inside the caller's transaction.
fn delete_outgoing(db: &Connection, source: &KnowledgeId) -> StorageResult<()> {
    let Some(source_id) = nodes_get_id_by_uri(db, source.as_str())? else {
        return Ok(());
    };
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Edges::Table)
            .and_where(Expr::col(Edges::SourceId).eq(source_id)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(UnresolvedEdges::Table)
            .and_where(Expr::col(UnresolvedEdges::SourceId).eq(source_id)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

/// Rejects node deletion while any owned declaration remains.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
/// * `source_id` — the internal row key of the node about to be deleted.
///
/// # Errors
/// Returns an invalid request for remaining declarations or reports database failures.
fn verify_no_outgoing(db: &Connection, source_id: i64) -> StorageResult<()> {
    let resolved = Query::select()
        .expr(Expr::val(1))
        .from(Edges::Table)
        .and_where(Expr::col(Edges::SourceId).eq(source_id))
        .to_owned();
    let unresolved = Query::select()
        .expr(Expr::val(1))
        .from(UnresolvedEdges::Table)
        .and_where(Expr::col(UnresolvedEdges::SourceId).eq(source_id))
        .to_owned();
    let (sql, values) =
        build_query(Query::select().expr(Expr::exists(resolved).or(Expr::exists(unresolved))))?;
    let is_outgoing: bool = db.query_row(&sql, params_from_iter(values), |row| row.get(0))?;
    if is_outgoing {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage declaration",
        ));
    }
    Ok(())
}

/// Preserves incoming declarations before deleting the target they reference.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
/// * `target_id` — the internal row key of the node being deleted.
/// * `target_uri` — the public address recorded for the preserved declarations.
///
/// # Errors
/// Reports mutation failures; the caller must roll back the transaction on error.
fn demote_incoming_edges(db: &Connection, target_id: i64, target_uri: &str) -> StorageResult<()> {
    let declarations = Query::select()
        .column(Edges::SourceId)
        .expr(Expr::val(target_uri))
        .column(Edges::Type)
        .from(Edges::Table)
        .and_where(Expr::col(Edges::TargetId).eq(target_id))
        .to_owned();
    let (sql, values) = build_query(
        Query::insert()
            .into_table(UnresolvedEdges::Table)
            .columns([
                UnresolvedEdges::SourceId,
                UnresolvedEdges::TargetUri,
                UnresolvedEdges::Type,
            ])
            .select_from(declarations)
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Edges::Table)
            .and_where(Expr::col(Edges::TargetId).eq(target_id)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

/// Resolves all declarations whose targets exist, without collecting graph rows in Rust.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
///
/// # Errors
/// Reports mutation failures; the caller must roll back rather than publish a revision.
fn resolve_edges(db: &Connection) -> StorageResult<()> {
    let declarations = Query::select()
        .column((UnresolvedEdges::Table, UnresolvedEdges::SourceId))
        .column((Nodes::Table, Nodes::Id))
        .column((UnresolvedEdges::Table, UnresolvedEdges::Type))
        .from(UnresolvedEdges::Table)
        .inner_join(
            Nodes::Table,
            Expr::col((Nodes::Table, Nodes::Uri))
                .equals((UnresolvedEdges::Table, UnresolvedEdges::TargetUri)),
        )
        .to_owned();
    let (sql, values) = build_query(
        Query::insert()
            .into_table(Edges::Table)
            .columns([Edges::SourceId, Edges::TargetId, Edges::Type])
            .select_from(declarations)
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let targets = Query::select()
        .column(Nodes::Uri)
        .from(Nodes::Table)
        .to_owned();
    let (sql, values) = build_query(
        Query::delete()
            .from_table(UnresolvedEdges::Table)
            .and_where(Expr::col(UnresolvedEdges::TargetUri).in_subquery(targets)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

fn verify_no_duplicate(
    db: &Connection,
    source_id: i64,
    target_uri: &str,
    r#type: &str,
) -> StorageResult<()> {
    let resolved = Query::select()
        .expr(Expr::val(1))
        .from(Edges::Table)
        .inner_join(
            Nodes::Table,
            Expr::col((Nodes::Table, Nodes::Id)).equals((Edges::Table, Edges::TargetId)),
        )
        .and_where(Expr::col(Edges::SourceId).eq(source_id))
        .and_where(Expr::col(Nodes::Uri).eq(target_uri))
        .and_where(Expr::col(Edges::Type).eq(r#type))
        .to_owned();
    let unresolved = Query::select()
        .expr(Expr::val(1))
        .from(UnresolvedEdges::Table)
        .and_where(Expr::col(UnresolvedEdges::SourceId).eq(source_id))
        .and_where(Expr::col(UnresolvedEdges::TargetUri).eq(target_uri))
        .and_where(Expr::col(UnresolvedEdges::Type).eq(r#type))
        .to_owned();
    let (sql, values) =
        build_query(Query::select().expr(Expr::exists(resolved).or(Expr::exists(unresolved))))?;
    let is_duplicate: bool = db.query_row(&sql, params_from_iter(values), |row| row.get(0))?;
    if is_duplicate {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage declaration",
        ));
    }
    Ok(())
}

fn build_resolved_query() -> SelectStatement {
    Query::select()
        .expr_as(
            Expr::col((Endpoint::Source, Nodes::Uri)),
            Declaration::SourceUri,
        )
        .expr_as(
            Expr::col((Endpoint::Target, Nodes::Uri)),
            Declaration::TargetUri,
        )
        .expr_as(Expr::col((Edges::Table, Edges::Type)), Declaration::Type)
        .from(Edges::Table)
        .join_as(
            JoinType::InnerJoin,
            Nodes::Table,
            Endpoint::Source,
            Expr::col((Endpoint::Source, Nodes::Id)).equals((Edges::Table, Edges::SourceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            Nodes::Table,
            Endpoint::Target,
            Expr::col((Endpoint::Target, Nodes::Id)).equals((Edges::Table, Edges::TargetId)),
        )
        .to_owned()
}

fn build_unresolved_query() -> SelectStatement {
    Query::select()
        .expr_as(
            Expr::col((Endpoint::Source, Nodes::Uri)),
            Declaration::SourceUri,
        )
        .expr_as(
            Expr::col(UnresolvedEdges::TargetUri),
            Declaration::TargetUri,
        )
        .expr_as(Expr::col(UnresolvedEdges::Type), Declaration::Type)
        .from(UnresolvedEdges::Table)
        .join_as(
            JoinType::InnerJoin,
            Nodes::Table,
            Endpoint::Source,
            Expr::col((Endpoint::Source, Nodes::Id))
                .equals((UnresolvedEdges::Table, UnresolvedEdges::SourceId)),
        )
        .to_owned()
}

fn decode_edge(row: &Row<'_>) -> StorageResult<Edge> {
    let source: String = row.get(0)?;
    let target: String = row.get(1)?;
    let edge_type: String = row.get(2)?;
    let target = if edge_type == "external" {
        EdgeTarget::Source(SourceUri::parse(&target).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?)
    } else {
        EdgeTarget::Knowledge(KnowledgeId::parse(&target).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?)
    };
    Edge::new(
        KnowledgeId::parse(&source).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
        target,
        EdgeType::parse(&edge_type).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
    )
    .map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InternalError,
            "invalid persisted storage value",
            error,
        )
    })
}

/// Deletes an existing node after checking owned declarations and preserving incoming links.
///
/// The caller owns the transaction. Missing nodes are a no-op; successful deletion
/// preserves incoming declarations before removing the node and its text index row.
///
/// # Arguments
///
/// * `db` — the write connection inside the caller's transaction.
/// * `id` — the existing element to delete, matched by its public address.
///
/// # Errors
/// Rejects nodes with outgoing declarations and reports mutation failures.
fn delete_node(db: &Connection, id: &EdgeTarget) -> StorageResult<()> {
    let Some(node_id) = nodes_get_id_by_uri(db, id.as_str())? else {
        return Ok(());
    };
    verify_no_outgoing(db, node_id)?;
    demote_incoming_edges(db, node_id, id.as_str())?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(TextIndex::Table)
            .and_where(Expr::col(TextIndex::Rowid).eq(node_id)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Nodes::Table)
            .and_where(Expr::col(Nodes::Id).eq(node_id)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

/// Returns the internal key for an existing public address without decoding metadata.
///
/// # Arguments
///
/// * `db` — the read connection holding an active scope.
/// * `uri` — the public address to look up.
///
/// # Errors
/// Reports query or database failures.
fn nodes_get_id_by_uri(db: &Connection, uri: &str) -> StorageResult<Option<i64>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::Id)
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Uri).eq(uri)),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(StorageError::from)
}

fn map_mutation(error: rusqlite::Error) -> StorageError {
    if matches!(&error, rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::ConstraintViolation)
    {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "invalid storage mutation",
            error,
        )
    } else {
        StorageError::new_with_cause(StorageErrorKind::InternalError, STORAGE_FAILURE, error)
    }
}

fn map_match(error: rusqlite::Error) -> StorageError {
    if matches!(&error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::Unknown)
    {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "invalid full-text expression",
            error,
        )
    } else {
        StorageError::new_with_cause(StorageErrorKind::InternalError, STORAGE_FAILURE, error)
    }
}

fn edges_get_by_row_id_and_type(request: &NeighborPageRequest<'_>) -> SelectStatement {
    let (anchor, neighbor) = match request.direction {
        NeighborDirection::Outgoing => (Endpoint::Source, Endpoint::Target),
        NeighborDirection::Incoming => (Endpoint::Target, Endpoint::Source),
    };
    let mut query = Query::select();
    query
        .columns([Nodes::Id, Nodes::Uri, Nodes::Kind].map(|col| (neighbor, col)))
        .column((Endpoint::Source, Nodes::Uri))
        .column((Endpoint::Target, Nodes::Uri))
        .column((Edges::Table, Edges::Type))
        .column((Endpoint::Target, Nodes::Kind))
        .from(Edges::Table)
        .join_as(
            JoinType::InnerJoin,
            Nodes::Table,
            Endpoint::Source,
            Expr::col((Endpoint::Source, Nodes::Id)).equals((Edges::Table, Edges::SourceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            Nodes::Table,
            Endpoint::Target,
            Expr::col((Endpoint::Target, Nodes::Id)).equals((Edges::Table, Edges::TargetId)),
        )
        .and_where(Expr::col((anchor, Nodes::Id)).eq(request.node.get_value()));
    apply_filters(&mut query, request, neighbor);
    query
        .order_by((Edges::Table, Edges::Type), Order::Asc)
        .order_by((neighbor, Nodes::Uri), Order::Asc)
        .limit(request.limit.get() as u64);
    query
}

fn apply_filters(
    query: &mut SelectStatement,
    request: &NeighborPageRequest<'_>,
    neighbor: Endpoint,
) {
    let kind = Expr::col((Edges::Table, Edges::Type));
    match request.mode {
        TraversalMode::Relations => {
            query.and_where(kind.clone().ne(INCLUDES_EDGE_TYPE));
        }
        TraversalMode::Containment => {
            query.and_where(kind.clone().eq(INCLUDES_EDGE_TYPE));
        }
        TraversalMode::Both => {}
    }
    if !request.edge_types.is_empty() {
        query.and_where(
            kind.clone()
                .is_in(request.edge_types.iter().map(EdgeType::as_str)),
        );
    }
    if let Some(cursor) = request.after {
        query.and_where(
            Expr::tuple([kind, Expr::col((neighbor, Nodes::Uri))]).gt(Expr::tuple([
                Expr::val(cursor.edge_type.as_str()),
                Expr::val(cursor.neighbor.as_str()),
            ])),
        );
    }
}

fn nodes_get_by_source_snapshot() -> SelectStatement {
    Query::select()
        .distinct()
        .column(Nodes::SourceSnapshot)
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::SourceSnapshot).is_not_null())
        .order_by(Nodes::SourceSnapshot, Order::Asc)
        .to_owned()
}

fn decode_neighbor(row: &Row<'_>) -> StorageResult<GraphNeighbor> {
    let source: String = row.get(3)?;
    let target: String = row.get(4)?;
    let edge_type: String = row.get(5)?;
    let target_kind: String = row.get(6)?;
    Ok(GraphNeighbor {
        node: decode_node_ref(row)?,
        edge: Edge::new(
            KnowledgeId::parse(&source).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
            decode_identity(&target, &target_kind)?,
            EdgeType::parse(&edge_type).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
        )
        .map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
    })
}

fn decode_node_ref(row: &Row<'_>) -> StorageResult<GraphNodeRef> {
    let id: String = row.get(1)?;
    let kind: String = row.get(2)?;
    Ok(GraphNodeRef {
        key: GraphNodeKey::new(row.get(0)?),
        element: decode_identity(&id, &kind)?,
    })
}

fn decode_identity(id: &str, kind: &str) -> StorageResult<EdgeTarget> {
    match kind {
        SOURCE_KIND => SourceUri::parse(id)
            .map(EdgeTarget::Source)
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            }),
        VERTEX_KIND | METAVERTEX_KIND => {
            KnowledgeId::parse(id)
                .map(EdgeTarget::Knowledge)
                .map_err(|error| {
                    StorageError::new_with_cause(
                        StorageErrorKind::InternalError,
                        "invalid persisted storage value",
                        error,
                    )
                })
        }
        _ => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored node kind",
        )),
    }
}

#[cfg(test)]
#[path = "cache_test.rs"]
mod tests;
