use super::{
    cache::{SOURCE_KIND, map_invalid_value},
    connection::map_sqlite,
    query::{
        EmbedFailures, Meta, Nodes, QueryErrorRow, Schemas, VectorIndex, VectorKeys, build_query,
    },
    transaction::{self, State},
};
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{
        EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary, EmbeddingWork, QueryError,
        StorageError, StorageErrorKind, StorageResult, StoredVectorKey, TransactionError,
        TransactionRequirement, VectorDatabase, VectorMatch, VectorOutcome,
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use sea_query::{Asterisk, Expr, ExprTrait, JoinType, OnConflict, Order, Query, SelectStatement};
use std::{cell::Cell, num::NonZeroUsize};

// Pinned vec1 0.8: VEC1_CONFIG_NELEM and VEC1_VECSIZE_MIN/MAX in vendor/vec1/vec1.c.
const NATIVE_DIMENSION_FIELD: i64 = 2;
const NATIVE_DIMENSION_MIN: usize = 2;
const NATIVE_DIMENSION_MAX: usize = 4096;

const RESET_INDEX: &str = r#"
    drop table vector_index;
    create virtual table vector_index using vec1(vector);
    insert into vector_index(cmd,arg) values('rebuild','{index:"none",distance:"cos"}');
"#;

/// Select the vector database on a write connection or a cache read connection.
#[derive(Debug, Clone, Copy)]
pub(super) enum VectorSchema {
    /// The vector connection's primary database.
    Main,
    /// The vector database attached to the cache connection as `vec`.
    Attached,
}

#[derive(Clone, Copy)]
enum HashCoverage {
    All,
    Stored,
}

#[derive(Debug)]
pub(crate) struct VectorStorage {
    pub(super) connection: Connection,
    state: Cell<State>,
}

impl VectorStorage {
    pub(crate) fn new(connection: Connection) -> Self {
        Self {
            connection,
            state: Cell::new(State::Idle),
        }
    }

    #[cfg(test)]
    pub(crate) fn get_test_connection(&self) -> &Connection {
        &self.connection
    }

    fn record<T>(&self, result: StorageResult<T>) -> StorageResult<T> {
        if result.is_err() {
            self.state.set(State::Failed);
        }
        result
    }

    fn get_write_connection(&self) -> StorageResult<&Connection> {
        if self.state.get() != State::Writing {
            if self.state.get() == State::Reading {
                // A caught write failure must still invalidate the enclosing scope.
                self.state.set(State::Failed);
            }
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "vector write scope is not active",
            ));
        }
        Ok(&self.connection)
    }

    fn get_connection(&self) -> StorageResult<&Connection> {
        match self.state.get() {
            State::Reading | State::Writing => Ok(&self.connection),
            _ => Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "vector scope is not active",
            )),
        }
    }
}

impl VectorDatabase for VectorStorage {
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()> {
        transaction::verify_scope(&self.state, requirement)
    }

    fn reset_vectors(
        &self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        if self.state.get() != State::Idle {
            if matches!(self.state.get(), State::Reading | State::Writing) {
                self.state.set(State::Failed);
            }
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cannot reset vectors inside a transaction",
            ));
        }
        let result = reset_vectors(&self.connection, observed, desired);
        if self.connection.is_autocommit() {
            return result;
        }
        // Guard Drop cannot report rollback failure on a no-op or identity mismatch.
        self.state.set(State::Broken);
        match result {
            Err(error) => Err(error),
            Ok(_) => Err(StorageError::new(
                StorageErrorKind::InternalError,
                "vector reset transaction did not close",
            )),
        }
    }

    fn vector_index_config_get_by_id(&self) -> StorageResult<Option<NonZeroUsize>> {
        vector_index_config_get_by_id(self.get_connection()?, VectorSchema::Main)
    }

    fn embed_failures_get_by_search_hash(
        &self,
        search_hash: &Sha256,
    ) -> StorageResult<Option<EmbeddingFailure>> {
        embed_failures_get_by_search_hash(self.get_connection()?, search_hash, VectorSchema::Main)
    }

    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>> {
        query_error_get_by_id(self.get_connection()?, VectorSchema::Main)
    }

    fn with_read_tx<T, E, F>(
        &self,
        expected: &Sha256,
        operation: F,
    ) -> Result<VectorOutcome<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        transaction::run_read(&self.connection, &self.state, || {
            if !matches_identity(
                self.get_connection().map_err(TransactionError::Storage)?,
                expected,
            )
            .map_err(TransactionError::Storage)?
            {
                return Ok(VectorOutcome::ConfigChanged);
            }
            operation()
                .map(VectorOutcome::Current)
                .map_err(TransactionError::Operation)
        })
        .map_err(TransactionError::flatten)
    }

    fn with_write_tx<T, E, F>(
        &self,
        expected: &Sha256,
        operation: F,
    ) -> Result<VectorOutcome<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let result = transaction::run(
            &self.connection,
            &self.state,
            || {
                if !matches_identity(
                    self.get_write_connection()
                        .map_err(TransactionError::Storage)?,
                    expected,
                )
                .map_err(TransactionError::Storage)?
                {
                    return Ok(VectorOutcome::ConfigChanged);
                }
                operation()
                    .map(VectorOutcome::Current)
                    .map_err(TransactionError::Operation)
            },
            || Ok(()),
        );
        result.map_err(TransactionError::flatten)
    }

    fn prune_unreferenced(&self) -> StorageResult<()> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let orphan_keys = Query::select()
                .column(VectorKeys::RowId)
                .from(VectorKeys::Table)
                .and_where(
                    Expr::exists(nodes_get_by_kind_and_search_hash(Expr::col((
                        VectorKeys::Table,
                        VectorKeys::SearchHash,
                    ))))
                    .not(),
                )
                .to_owned();
            let (sql, values) = build_query(
                Query::delete()
                    .from_table(VectorIndex::Table)
                    .and_where(Expr::col(VectorIndex::Rowid).in_subquery(orphan_keys)),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            let (sql, values) = build_query(
                Query::delete().from_table(VectorKeys::Table).and_where(
                    Expr::exists(nodes_get_by_kind_and_search_hash(Expr::col((
                        VectorKeys::Table,
                        VectorKeys::SearchHash,
                    ))))
                    .not(),
                ),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            let (sql, values) = build_query(
                Query::delete().from_table(EmbedFailures::Table).and_where(
                    Expr::exists(nodes_get_by_kind_and_search_hash(Expr::col((
                        EmbedFailures::Table,
                        EmbedFailures::SearchHash,
                    ))))
                    .not(),
                ),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            Ok(())
        })();
        self.record(result)
    }

    fn embed_failures_upsert_by_search_hash_and_attempts(
        &self,
        search_hash: &Sha256,
        max_attempts: u64,
    ) -> StorageResult<bool> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let budget = encode_unsigned(max_attempts)?;
            if budget == 0 {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidRequest,
                    "attempt budget must be positive",
                ));
            }
            let (sql, values) = build_query(
                Query::insert()
                    .into_table(EmbedFailures::Table)
                    .columns([EmbedFailures::SearchHash, EmbedFailures::Attempts])
                    .values([Expr::val(search_hash.to_hex()), Expr::val(1)])
                    .map_err(map_invalid_value)?
                    .on_conflict(
                        OnConflict::column(EmbedFailures::SearchHash)
                            .value(
                                EmbedFailures::Attempts,
                                Expr::col(EmbedFailures::Attempts).add(1),
                            )
                            .action_and_where(Expr::col(EmbedFailures::Attempts).lt(budget))
                            .to_owned(),
                    ),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map(|changed| changed == 1)
                .map_err(map_sqlite)
        })();
        self.record(result)
    }

    fn embed_failures_update_by_search_hash(
        &self,
        search_hash: &Sha256,
        error: &str,
    ) -> StorageResult<bool> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let (sql, values) = build_query(
                Query::update()
                    .table(EmbedFailures::Table)
                    .value(EmbedFailures::Error, error)
                    .and_where(Expr::col(EmbedFailures::SearchHash).eq(search_hash.to_hex())),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map(|changed| changed == 1)
                .map_err(map_sqlite)
        })();
        self.record(result)
    }

    fn query_error_upsert_by_id(&self, at: u64, error: &str) -> StorageResult<()> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let at = encode_unsigned(at)?;
            let (sql, values) = build_query(
                Query::insert()
                    .into_table(QueryErrorRow::Table)
                    .columns([QueryErrorRow::Id, QueryErrorRow::At, QueryErrorRow::Error])
                    .values([Expr::val(1), Expr::val(at), Expr::val(error)])
                    .map_err(map_invalid_value)?
                    .on_conflict(
                        OnConflict::column(QueryErrorRow::Id)
                            .update_columns([QueryErrorRow::At, QueryErrorRow::Error])
                            .to_owned(),
                    ),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            Ok(())
        })();
        self.record(result)
    }

    fn query_error_delete_by_id(&self) -> StorageResult<()> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let (sql, values) = build_query(
                Query::delete()
                    .from_table(QueryErrorRow::Table)
                    .and_where(Expr::col(QueryErrorRow::Id).eq(1)),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            Ok(())
        })();
        self.record(result)
    }

    fn store_vector(
        &self,
        search_hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey> {
        let connection = self.get_write_connection()?;
        let result = (|| {
            let bytes = encode_vector(vector, dimensions)?;
            validate_dimensions(connection, dimensions)?;
            let inserted = vector_keys_upsert_by_search_hash(connection, search_hash)?;
            let row_id = vector_keys_get_by_search_hash(connection, search_hash)?;
            let key = StoredVectorKey { row_id, inserted };
            if key.inserted {
                let (sql, values) = build_query(
                    Query::insert()
                        .into_table(VectorIndex::Table)
                        .columns([VectorIndex::Rowid, VectorIndex::Vector])
                        .values([Expr::val(key.row_id), Expr::val(bytes)])
                        .map_err(map_invalid_value)?,
                )?;
                connection
                    .execute(&sql, params_from_iter(values))
                    .map_err(map_sqlite)?;
            } else {
                let (sql, values) = build_query(
                    Query::select()
                        .expr(Expr::val(1))
                        .from(VectorIndex::Table)
                        .and_where(Expr::col(VectorIndex::Rowid).eq(key.row_id)),
                )?;
                let exists = connection
                    .query_row(&sql, params_from_iter(values), |_| Ok(()))
                    .optional()
                    .map_err(map_sqlite)?;
                if exists.is_none() {
                    return Err(StorageError::new(
                        StorageErrorKind::InternalError,
                        "invalid stored vector data",
                    ));
                }
            }
            let (sql, values) = build_query(
                Query::delete()
                    .from_table(EmbedFailures::Table)
                    .and_where(Expr::col(EmbedFailures::SearchHash).eq(search_hash.to_hex())),
            )?;
            connection
                .execute(&sql, params_from_iter(values))
                .map_err(map_sqlite)?;
            Ok(key)
        })();
        self.record(result)
    }
}

fn reset_vectors(
    conn: &Connection,
    observed: Option<&Sha256>,
    identity: &Sha256,
) -> StorageResult<VectorOutcome<bool>> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(map_sqlite)?;
    let current = meta_get_by_id(&tx, VectorSchema::Main)?;
    if current.as_ref() == Some(identity) {
        return Ok(VectorOutcome::Current(false));
    }
    if current.as_ref() != observed {
        return Ok(VectorOutcome::ConfigChanged);
    }
    tx.execute_batch(RESET_INDEX).map_err(map_sqlite)?;
    let (sql, values) = build_query(Query::delete().from_table(VectorKeys::Table))?;
    tx.execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    let (sql, values) = build_query(Query::delete().from_table(EmbedFailures::Table))?;
    tx.execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    let (sql, values) = build_query(
        Query::insert()
            .into_table(Meta::Table)
            .columns([Meta::Id, Meta::ConfigIdentity])
            .values([Expr::val(1), Expr::val(identity.to_hex())])
            .map_err(map_invalid_value)?
            .on_conflict(
                OnConflict::column(Meta::Id)
                    .update_column(Meta::ConfigIdentity)
                    .to_owned(),
            ),
    )?;
    tx.execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    tx.commit().map_err(map_sqlite)?;
    Ok(VectorOutcome::Current(true))
}

pub(super) fn meta_get_by_id(
    connection: &Connection,
    schema: VectorSchema,
) -> StorageResult<Option<Sha256>> {
    let mut query = Query::select();
    query
        .column(Meta::ConfigIdentity)
        .and_where(Expr::col(Meta::Id).eq(1));
    match schema {
        VectorSchema::Main => query.from(Meta::Table),
        VectorSchema::Attached => query.from((Schemas::Vec, Meta::Table)),
    };
    let (sql, values) = build_query(&query)?;
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

fn matches_identity(connection: &Connection, expected: &Sha256) -> StorageResult<bool> {
    let (sql, values) = build_query(
        Query::select()
            .column(Meta::ConfigIdentity)
            .from(Meta::Table)
            .and_where(Expr::col(Meta::Id).eq(1)),
    )?;
    let identity: Option<String> = connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .optional()
        .map_err(map_sqlite)?;
    Ok(identity.as_deref() == Some(expected.to_hex().as_str()))
}

fn validate_dimensions(connection: &Connection, dimensions: usize) -> StorageResult<()> {
    let current = vector_index_config_get_by_id(connection, VectorSchema::Main)?;
    if !(NATIVE_DIMENSION_MIN..=NATIVE_DIMENSION_MAX).contains(&dimensions)
        || current.is_some_and(|size| size.get() != dimensions)
    {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "unsupported or mismatched vector dimensions",
        ));
    }
    Ok(())
}

pub(super) fn vector_index_config_get_by_id(
    connection: &Connection,
    schema: VectorSchema,
) -> StorageResult<Option<NonZeroUsize>> {
    // Native shadow introspection is read-only, with private fixed schema names.
    let sql = match schema {
        VectorSchema::Main => "select val from main.vector_index_config where id = ?1",
        VectorSchema::Attached => "select val from vec.vector_index_config where id = ?1",
    };
    let value: Option<i64> = connection
        .query_row(sql, [NATIVE_DIMENSION_FIELD], |row| row.get(0))
        .optional()
        .map_err(map_sqlite)?;
    value
        .map(|value| {
            let size = usize::try_from(value).map_err(|_| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                )
            })?;
            if !(NATIVE_DIMENSION_MIN..=NATIVE_DIMENSION_MAX).contains(&size) {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                ));
            }
            NonZeroUsize::new(size).ok_or_else(|| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                )
            })
        })
        .transpose()
}

fn vector_keys_upsert_by_search_hash(
    connection: &Connection,
    search_hash: &Sha256,
) -> StorageResult<bool> {
    let (sql, values) = build_query(
        Query::insert()
            .into_table(VectorKeys::Table)
            .columns([VectorKeys::SearchHash])
            .values([Expr::val(search_hash.to_hex())])
            .map_err(map_invalid_value)?
            .on_conflict(
                OnConflict::column(VectorKeys::SearchHash)
                    .do_nothing()
                    .to_owned(),
            ),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map(|changed| changed == 1)
        .map_err(map_sqlite)
}

fn vector_keys_get_by_search_hash(
    connection: &Connection,
    search_hash: &Sha256,
) -> StorageResult<i64> {
    let (sql, values) = build_query(
        Query::select()
            .column(VectorKeys::RowId)
            .from(VectorKeys::Table)
            .and_where(Expr::col(VectorKeys::SearchHash).eq(search_hash.to_hex())),
    )?;
    connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .map_err(map_sqlite)
}

pub(super) fn vector_keys_get_vector_by_search_hash(
    connection: &Connection,
    search_hash: &Sha256,
) -> StorageResult<Option<Vec<f32>>> {
    let (sql, values) = build_query(
        Query::select()
            .column((VectorIndex::Table, VectorIndex::Vector))
            .from((Schemas::Vec, VectorKeys::Table))
            .join(
                JoinType::LeftJoin,
                (Schemas::Vec, VectorIndex::Table),
                Expr::col((VectorIndex::Table, VectorIndex::Rowid))
                    .equals((VectorKeys::Table, VectorKeys::RowId)),
            )
            .and_where(
                Expr::col((VectorKeys::Table, VectorKeys::SearchHash)).eq(search_hash.to_hex()),
            ),
    )?;
    let value = connection
        .query_row(&sql, params_from_iter(values), |r| {
            r.get::<_, Option<Vec<u8>>>(0)
        })
        .optional()
        .map_err(map_sqlite)?;
    value
        .map(|bytes| {
            decode_vector(&bytes.ok_or_else(|| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                )
            })?)
        })
        .transpose()
}

fn decode_vector(bytes: &[u8]) -> StorageResult<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored vector data",
        ));
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored vector data",
        ));
    }
    Ok(values)
}

pub(super) fn get_nearest_vectors(
    connection: &Connection,
    query: &[f32],
    dimensions: usize,
) -> StorageResult<Vec<VectorMatch>> {
    let bytes = encode_vector(query, dimensions)?;
    let count = vector_keys_get_count(connection)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    vector_index_get_by_vector(connection, &bytes, count)
}

fn vector_keys_get_count(connection: &Connection) -> StorageResult<i64> {
    let (sql, values) = build_query(
        Query::select()
            .expr(Expr::col(Asterisk).count())
            .from((Schemas::Vec, VectorKeys::Table)),
    )?;
    connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .map_err(map_sqlite)
}

fn vector_index_get_by_vector(
    connection: &Connection,
    bytes: &[u8],
    count: i64,
) -> StorageResult<Vec<VectorMatch>> {
    let mut statement = connection
        .prepare(
            "select n.id, k.search_hash, v.distance
         from vec.vector_index(?1, ?2) v
         join vec.vector_keys k on k.row_id = v.rowid
         join nodes n on n.search_hash = k.search_hash
         where n.kind <> ?3
         order by v.distance, n.id",
        )
        .map_err(map_sqlite)?;
    let rows = statement
        .query_map(params![bytes, format!("{{K:{count}}}"), SOURCE_KIND], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .map_err(map_sqlite)?;
    rows.map(|row| {
        let (id, search_hash, distance) = row.map_err(map_sqlite)?;
        Ok(VectorMatch {
            id: KnowledgeId::parse(&id).map_err(|_| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                )
            })?,
            search_hash: Sha256::parse(&search_hash).map_err(|_| {
                StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector data",
                )
            })?,
            distance,
        })
    })
    .collect()
}

fn encode_vector(vector: &[f32], dimensions: usize) -> StorageResult<Vec<u8>> {
    if vector.is_empty() || vector.len() != dimensions || vector.iter().any(|v| !v.is_finite()) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector values or dimensions",
        ));
    }
    Ok(vector.iter().flat_map(|v| v.to_ne_bytes()).collect())
}

fn encode_unsigned(value: u64) -> StorageResult<i64> {
    i64::try_from(value).map_err(|cause| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "unsupported diagnostic number",
            cause,
        )
    })
}

pub(super) fn embed_failures_get_by_search_hash(
    connection: &Connection,
    search_hash: &Sha256,
    schema: VectorSchema,
) -> StorageResult<Option<EmbeddingFailure>> {
    let mut query = Query::select();
    query.columns([EmbedFailures::Attempts, EmbedFailures::Error]);
    match schema {
        VectorSchema::Main => query.from(EmbedFailures::Table),
        VectorSchema::Attached => query.from((Schemas::Vec, EmbedFailures::Table)),
    };
    query.and_where(Expr::col(EmbedFailures::SearchHash).eq(search_hash.to_hex()));
    let (sql, values) = build_query(&query)?;
    let value = connection
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .optional()
        .map_err(map_sqlite)?;
    value
        .map(|(attempts, error)| {
            let attempts = decode_unsigned(attempts)?;
            if attempts == 0 {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored diagnostic",
                ));
            }
            Ok(EmbeddingFailure {
                search_hash: *search_hash,
                attempts,
                error,
            })
        })
        .transpose()
}

pub(super) fn query_error_get_by_id(
    connection: &Connection,
    schema: VectorSchema,
) -> StorageResult<Option<QueryError>> {
    let mut query = Query::select();
    query.columns([QueryErrorRow::At, QueryErrorRow::Error]);
    match schema {
        VectorSchema::Main => query.from(QueryErrorRow::Table),
        VectorSchema::Attached => query.from((Schemas::Vec, QueryErrorRow::Table)),
    };
    query.and_where(Expr::col(QueryErrorRow::Id).eq(1));
    let (sql, values) = build_query(&query)?;
    let value = connection
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .optional()
        .map_err(map_sqlite)?;
    value
        .map(|(at, error)| {
            Ok(QueryError {
                at: decode_unsigned(at)?,
                error,
            })
        })
        .transpose()
}

fn decode_unsigned(value: i64) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| {
        StorageError::new(StorageErrorKind::InternalError, "invalid stored diagnostic")
    })
}

pub(super) fn nodes_get_by_kind_and_search_hash_and_attempts_and_id(
    connection: &Connection,
    max_attempts: u64,
    id: Option<&KnowledgeId>,
) -> StorageResult<Option<EmbeddingWork>> {
    if max_attempts == 0 {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "attempt budget must be positive",
        ));
    }
    let mut query = Query::select();
    query
        .columns([Nodes::Id, Nodes::SearchHash])
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
        .and_where(Expr::exists(vector_keys_get_existence_by_search_hash()).not())
        .and_where(Expr::exists(embed_failures_get_by_search_hash_and_attempts(max_attempts)).not())
        .order_by(Nodes::Id, Order::Asc)
        .limit(1);
    if let Some(id) = id {
        query.and_where(Expr::col(Nodes::Id).gt(id.as_str()));
    }
    let (sql, values) = build_query(&query)?;
    let row = connection
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()
        .map_err(map_sqlite)?;
    row.map(|(id, hash)| {
        Ok(EmbeddingWork {
            id: KnowledgeId::parse(&id).map_err(map_invalid_value)?,
            search_hash: Sha256::parse(&hash).map_err(map_invalid_value)?,
        })
    })
    .transpose()
}

fn embed_failures_get_by_search_hash_and_attempts(max_attempts: u64) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Vec, EmbedFailures::Table))
        .and_where(
            Expr::col((EmbedFailures::Table, EmbedFailures::SearchHash))
                .equals((Nodes::Table, Nodes::SearchHash)),
        )
        .and_where(Expr::col(EmbedFailures::Attempts).gte(max_attempts))
        .to_owned()
}

fn nodes_get_by_kind_and_search_hash(hash: Expr) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Cache, Nodes::Table))
        .and_where(Expr::col((Nodes::Table, Nodes::Kind)).ne(SOURCE_KIND))
        .and_where(Expr::col((Nodes::Table, Nodes::SearchHash)).eq(hash))
        .to_owned()
}

pub(super) fn get_embedding_status(
    connection: &Connection,
    identity: Option<Sha256>,
    policy: &EmbeddingStatusPolicy,
) -> StorageResult<EmbeddingStatusSummary> {
    if matches!(policy, EmbeddingStatusPolicy::Disabled) {
        return Ok(EmbeddingStatusSummary {
            backlog: 0,
            failed: 0,
            exhausted: vec![],
        });
    }
    let total = nodes_get_count_by_kind_and_search_hash(connection, HashCoverage::All)?;
    let EmbeddingStatusPolicy::Ready {
        identity: expected,
        max_attempts,
    } = policy
    else {
        return Ok(EmbeddingStatusSummary {
            backlog: total,
            failed: 0,
            exhausted: vec![],
        });
    };
    if *max_attempts == 0 || i64::try_from(*max_attempts).is_err() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid status attempt budget",
        ));
    }
    if identity != Some(*expected) {
        return Ok(EmbeddingStatusSummary {
            backlog: total,
            failed: 0,
            exhausted: vec![],
        });
    }
    verify_current_vectors(connection)?;
    let covered = nodes_get_count_by_kind_and_search_hash(connection, HashCoverage::Stored)?;
    let exhausted =
        embed_failures_get_by_kind_and_search_hash_and_attempts(connection, *max_attempts)?;
    let failed = u64::try_from(exhausted.len()).map_err(map_invalid_value)?;
    let backlog = total
        .checked_sub(covered)
        .and_then(|missing| missing.checked_sub(failed))
        .ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "invalid embedding status counts",
            )
        })?;
    Ok(EmbeddingStatusSummary {
        backlog,
        failed,
        exhausted,
    })
}

fn verify_current_vectors(connection: &Connection) -> StorageResult<()> {
    if vector_keys_get_missing_by_rowid_and_kind_and_search_hash(connection)?.is_some() {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored vector data",
        ));
    }
    Ok(())
}

fn vector_keys_get_missing_by_rowid_and_kind_and_search_hash(
    connection: &Connection,
) -> StorageResult<Option<i64>> {
    let (sql, values) = build_query(
        Query::select()
            .column((VectorKeys::Table, VectorKeys::RowId))
            .from((Schemas::Vec, VectorKeys::Table))
            .join(
                JoinType::LeftJoin,
                (Schemas::Vec, VectorIndex::Table),
                Expr::col((VectorIndex::Table, VectorIndex::Rowid))
                    .equals((VectorKeys::Table, VectorKeys::RowId)),
            )
            .and_where(Expr::col((VectorIndex::Table, VectorIndex::Rowid)).is_null())
            .and_where(Expr::exists(
                Query::select()
                    .expr(Expr::val(1))
                    .from(Nodes::Table)
                    .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
                    .and_where(
                        Expr::col((Nodes::Table, Nodes::SearchHash))
                            .equals((VectorKeys::Table, VectorKeys::SearchHash)),
                    )
                    .to_owned(),
            ))
            .limit(1),
    )?;
    connection
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(map_sqlite)
}

fn nodes_get_count_by_kind_and_search_hash(
    connection: &Connection,
    coverage: HashCoverage,
) -> StorageResult<u64> {
    let mut query = Query::select();
    query
        .expr(Expr::col(Nodes::SearchHash).count_distinct())
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND));
    if matches!(coverage, HashCoverage::Stored) {
        query.and_where(Expr::exists(vector_keys_get_existence_by_search_hash()));
    }
    let (sql, values) = build_query(&query)?;
    let count: i64 = connection
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .map_err(map_sqlite)?;
    u64::try_from(count).map_err(map_invalid_value)
}

fn embed_failures_get_by_kind_and_search_hash_and_attempts(
    connection: &Connection,
    max_attempts: u64,
) -> StorageResult<Vec<EmbeddingFailure>> {
    let (sql, values) = build_query(
        Query::select()
            .distinct()
            .columns([
                EmbedFailures::SearchHash,
                EmbedFailures::Attempts,
                EmbedFailures::Error,
            ])
            .from((Schemas::Vec, EmbedFailures::Table))
            .and_where(Expr::exists(
                Query::select()
                    .expr(Expr::val(1))
                    .from(Nodes::Table)
                    .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
                    .and_where(
                        Expr::col((Nodes::Table, Nodes::SearchHash))
                            .equals((EmbedFailures::Table, EmbedFailures::SearchHash)),
                    )
                    .and_where(Expr::exists(vector_keys_get_existence_by_search_hash()).not())
                    .to_owned(),
            ))
            .order_by(EmbedFailures::SearchHash, Order::Asc),
    )?;
    let mut statement = connection.prepare(&sql).map_err(map_sqlite)?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(map_sqlite)?;
    let mut exhausted = Vec::new();
    for row in rows {
        let (hash, attempts, error) = row.map_err(map_sqlite)?;
        let attempts = u64::try_from(attempts).map_err(map_invalid_value)?;
        if attempts == 0 {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "invalid stored embedding attempts",
            ));
        }
        let search_hash = Sha256::parse(&hash).map_err(map_invalid_value)?;
        if attempts >= max_attempts {
            exhausted.push(EmbeddingFailure {
                search_hash,
                attempts,
                error,
            });
        }
    }
    Ok(exhausted)
}

fn vector_keys_get_existence_by_search_hash() -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Vec, VectorKeys::Table))
        .and_where(
            Expr::col((VectorKeys::Table, VectorKeys::SearchHash))
                .equals((Nodes::Table, Nodes::SearchHash)),
        )
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::VectorStorage;
    use crate::service::TransactionError;
    use crate::{
        domain::Sha256,
        service::{StorageError, VectorDatabase, VectorOutcome},
    };
    use rusqlite::Connection;

    #[test]
    fn failed_reset_cleanup_reports_failure_and_disables_the_connection() {
        use super::super::utils::set_rollback_denial;
        // Given: SQLite refuses rollback of a reset with an obsolete observation.
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("create table meta(id integer primary key,config_identity text)")
            .unwrap();
        let storage = VectorStorage::new(connection);
        let observed = Sha256::digest(b"obsolete observation");
        let desired = Sha256::digest(b"desired configuration");
        set_rollback_denial(&storage.connection, true);
        // When: reset skips its mutations but SQLite cannot close its transaction.
        let result = storage.reset_vectors(Some(&observed), &desired);
        set_rollback_denial(&storage.connection, false);
        storage.connection.execute_batch("rollback").unwrap();
        // Then: cleanup failure is an error and external repair cannot reenable this owner.
        assert!(result.is_err());
        let retry = storage.with_read_tx(&desired, || -> Result<(), StorageError> {
            panic!("disabled storage must not execute callbacks")
        });
        assert!(matches!(retry, Err(TransactionError::Storage(_))));
    }

    #[test]
    fn reset_is_rejected_inside_an_active_vector_scope() {
        // Given: a vector transaction for the current configuration.
        let connection = Connection::open_in_memory().unwrap();
        let identity = Sha256::digest(b"current");
        connection
            .execute_batch("create table meta(id integer primary key,config_identity text)")
            .unwrap();
        connection
            .execute("insert into meta values(1,?1)", [identity.to_hex()])
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: a service tries to reset identity from within its write scope.
        let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
            assert!(
                storage
                    .reset_vectors(Some(&identity), &Sha256::digest(b"other"))
                    .is_err()
            );
            Ok(())
        });
        // Then: the reset is rejected and the enclosing scope cannot commit.
        assert!(matches!(result, Err(TransactionError::Storage(_))));
    }

    #[test]
    fn reads_embedding_failure_from_vector_database_inside_scope() {
        // Given: a primary vector database with a recorded failed attempt.
        let connection = Connection::open_in_memory().unwrap();
        let identity = Sha256::digest(b"current");
        let hash = Sha256::digest(b"text");
        connection.execute_batch("create table meta(id integer primary key,config_identity text); create table embed_failures(search_hash text primary key,attempts integer,error text)").unwrap();
        connection
            .execute("insert into meta values(1,?1)", [identity.to_hex()])
            .unwrap();
        connection
            .execute(
                "insert into embed_failures values(?1,2,'safe failure')",
                [hash.to_hex()],
            )
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: a service reads the failure in a matching scope.
        let outcome = storage
            .with_read_tx(&identity, || {
                storage.embed_failures_get_by_search_hash(&hash)
            })
            .unwrap();
        // Then: the stored hash, attempt count and safe error are returned.
        let VectorOutcome::Current(Some(failure)) = outcome else {
            panic!("failure missing")
        };
        assert_eq!(failure.search_hash, hash);
        assert_eq!(failure.attempts, 2);
        assert_eq!(failure.error.as_deref(), Some("safe failure"));
    }

    #[test]
    fn reads_query_error_from_vector_database_inside_scope() {
        // Given: the primary vector database contains a query failure.
        let connection = Connection::open_in_memory().unwrap();
        let identity = Sha256::digest(b"current");
        connection.execute_batch("create table meta(id integer primary key,config_identity text); create table query_error(id integer primary key,at integer,error text); insert into query_error values(1,42,'safe failure')").unwrap();
        connection
            .execute("insert into meta values(1,?1)", [identity.to_hex()])
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: a service reads within the matching vector scope.
        let outcome = storage
            .with_read_tx(&identity, || storage.query_error_get_by_id())
            .unwrap();
        // Then: it receives the stored query error from the primary database.
        let VectorOutcome::Current(Some(error)) = outcome else {
            panic!("query error missing")
        };
        assert_eq!(error.at, 42);
        assert_eq!(error.error, "safe failure");
    }

    #[test]
    fn vector_read_scope_rejects_mismatched_identity_without_calling_operation() {
        // Given: a vector database configured for one identity.
        let connection = Connection::open_in_memory().unwrap();
        let current = Sha256::digest(b"current");
        connection
            .execute_batch("create table meta(id integer primary key,config_identity text)")
            .unwrap();
        connection
            .execute("insert into meta values(1,?1)", [current.to_hex()])
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: a read is requested for another configuration.
        let result = storage
            .with_read_tx(&Sha256::digest(b"other"), || -> Result<(), StorageError> {
                panic!("mismatched read callback ran")
            });
        // Then: the caller receives the ordinary changed-configuration outcome.
        assert!(matches!(result, Ok(VectorOutcome::ConfigChanged)));
    }

    #[test]
    fn caught_vector_write_error_rolls_back_earlier_changes() {
        // Given: a vector database with no query_error table.
        let connection = Connection::open_in_memory().unwrap();
        let identity = Sha256::digest(b"current");
        connection.execute_batch("create table meta(id integer primary key,config_identity text); create table sample(value integer)").unwrap();
        connection
            .execute("insert into meta values(1,?1)", [identity.to_hex()])
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: the caller catches a real failing vector write.
        let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
            storage
                .get_write_connection()?
                .execute_batch("insert into sample values(9)")
                .unwrap();
            assert!(storage.query_error_upsert_by_id(1, "safe").is_err());
            Ok(())
        });
        // Then: the transaction cannot commit the earlier write.
        assert!(matches!(result, Err(TransactionError::Storage(_))));
        assert_eq!(
            storage
                .connection
                .query_row("select count(*) from sample", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn incompatible_nested_identity_skips_callback_and_keeps_outer_work() {
        // Given: a vector database with one selected configuration identity.
        let connection = Connection::open_in_memory().unwrap();
        let identity = Sha256::digest(b"current");
        connection.execute_batch("create table meta(id integer primary key,config_identity text); create table sample(value integer)").unwrap();
        connection
            .execute("insert into meta values(1,?1)", [identity.to_hex()])
            .unwrap();
        let storage = VectorStorage::new(connection);
        // When: an incompatible nested request occurs inside compatible work.
        let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
            let skipped = storage
                .with_write_tx(&Sha256::digest(b"other"), || -> Result<(), StorageError> {
                    panic!("incompatible callback ran")
                })
                .unwrap();
            assert!(matches!(skipped, VectorOutcome::ConfigChanged));
            storage
                .get_write_connection()?
                .execute_batch("insert into sample values(9)")
                .unwrap();
            Ok(())
        });
        // Then: the compatible outer operation commits normally.
        assert!(matches!(result, Ok(VectorOutcome::Current(()))));
        assert_eq!(
            storage
                .connection
                .query_row("select value from sample", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
    }
}

#[cfg(test)]
mod query_tests {
    use super::{
        VectorSchema, VectorStorage, decode_vector, meta_get_by_id, vector_index_config_get_by_id,
    };
    use crate::{
        domain::Sha256,
        service::{StorageError, StorageErrorKind, VectorDatabase},
        storage::{native::register_vec1, schema::bootstrap_vectors},
    };
    use rusqlite::Connection;
    use tempfile::TempDir;

    #[test]
    fn gets_identity_and_dimensions_from_selected_schema() {
        // Given: independent main and attached databases have distinct vector metadata.
        let root = TempDir::new().unwrap();
        let path = root.path().join("attached.db");
        let main_identity = Sha256::digest(b"main");
        let attached_identity = Sha256::digest(b"attached");
        register_vec1().unwrap();
        let mut main = Connection::open_in_memory().unwrap();
        let mut attached = Connection::open(&path).unwrap();
        bootstrap_vectors(&mut main).unwrap();
        bootstrap_vectors(&mut attached).unwrap();
        let main = VectorStorage::new(main);
        let attached = VectorStorage::new(attached);
        for (storage, identity, vector) in [
            (&main, &main_identity, vec![1.0, 0.0]),
            (&attached, &attached_identity, vec![1.0, 0.0, 0.0]),
        ] {
            storage.reset_vectors(None, identity).unwrap();
            storage
                .with_write_tx(identity, || -> Result<_, StorageError> {
                    storage.store_vector(identity, &vector, vector.len())
                })
                .unwrap();
        }
        let main = main.get_test_connection();
        main.execute("attach database ?1 as vec", [path.to_str().unwrap()])
            .unwrap();

        // When: metadata is retrieved through each explicit schema selector.
        let main_id = meta_get_by_id(main, VectorSchema::Main).unwrap();
        let attached_id = meta_get_by_id(main, VectorSchema::Attached).unwrap();
        let main_dimensions = vector_index_config_get_by_id(main, VectorSchema::Main).unwrap();
        let attached_dimensions =
            vector_index_config_get_by_id(main, VectorSchema::Attached).unwrap();

        // Then: neither identity nor native dimensions leak across schema locations.
        assert_eq!(main_id, Some(main_identity));
        assert_eq!(attached_id, Some(attached_identity));
        assert_eq!(main_dimensions.unwrap().get(), 2);
        assert_eq!(attached_dimensions.unwrap().get(), 3);
    }

    #[test]
    fn rejects_empty_or_partially_encoded_stored_vectors() {
        // Given: retain a valid component before a truncated trailing component.
        let mut truncated = 1.0_f32.to_ne_bytes().to_vec();
        truncated.push(0);
        for bytes in [Vec::new(), vec![0], truncated] {
            // When: malformed stored bytes are decoded.
            let error = decode_vector(&bytes).unwrap_err();
            // Then: incomplete bytes cannot silently become a shorter vector.
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        }
        // When / Then: complete finite components still decode without loss.
        assert_eq!(decode_vector(&1.0_f32.to_ne_bytes()).unwrap(), vec![1.0]);
    }
}
