use super::{
    cache::SOURCE_KIND,
    connection::STORAGE_FAILURE,
    query::{
        EmbedFailures, Meta, Nodes, QueryErrorRow, Schemas, VectorIndex, VectorKeys, build_query,
    },
    transaction::{self, State},
};
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{
        EmbeddingFailure, EmbeddingWork, QueryError, StorageError, StorageErrorKind, StorageResult,
        StoredVectorKey, TransactionError, TransactionRequirement, VectorDatabase, VectorMatch,
        VectorOutcome,
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
    insert into vector_index(cmd,arg) values('rebuild','{index:"flat",distance:"cos"}');
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

    fn vector_keys_get_by_search_hash(&self, search_hash: &Sha256) -> StorageResult<i64> {
        let connection = self.get_write_connection()?;
        let (sql, values) = build_query(
            Query::select()
                .column(VectorKeys::RowId)
                .from(VectorKeys::Table)
                .and_where(Expr::col(VectorKeys::SearchHash).eq(search_hash.to_hex())),
        )?;
        connection
            .query_row(&sql, params_from_iter(values), |r| r.get(0))
            .map_err(StorageError::from)
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
            if meta_get_by_id(
                self.get_connection().map_err(TransactionError::Storage)?,
                VectorSchema::Main,
            )
            .map_err(TransactionError::Storage)?
            .as_deref()
                != Some(expected.to_hex().as_str())
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
                if meta_get_by_id(
                    self.get_write_connection()
                        .map_err(TransactionError::Storage)?,
                    VectorSchema::Main,
                )
                .map_err(TransactionError::Storage)?
                .as_deref()
                    != Some(expected.to_hex().as_str())
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
                    Expr::exists(build_current_nodes_query(Expr::col((
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
            connection.execute(&sql, params_from_iter(values))?;
            let (sql, values) = build_query(
                Query::delete().from_table(VectorKeys::Table).and_where(
                    Expr::exists(build_current_nodes_query(Expr::col((
                        VectorKeys::Table,
                        VectorKeys::SearchHash,
                    ))))
                    .not(),
                ),
            )?;
            connection.execute(&sql, params_from_iter(values))?;
            let (sql, values) = build_query(
                Query::delete().from_table(EmbedFailures::Table).and_where(
                    Expr::exists(build_current_nodes_query(Expr::col((
                        EmbedFailures::Table,
                        EmbedFailures::SearchHash,
                    ))))
                    .not(),
                ),
            )?;
            connection.execute(&sql, params_from_iter(values))?;
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
                    .map_err(|error| {
                        StorageError::new_with_cause(
                            StorageErrorKind::InternalError,
                            "invalid persisted storage value",
                            error,
                        )
                    })?
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
                .map_err(StorageError::from)
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
                .map_err(StorageError::from)
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
                    .map_err(|error| {
                        StorageError::new_with_cause(
                            StorageErrorKind::InternalError,
                            "invalid persisted storage value",
                            error,
                        )
                    })?
                    .on_conflict(
                        OnConflict::column(QueryErrorRow::Id)
                            .update_columns([QueryErrorRow::At, QueryErrorRow::Error])
                            .to_owned(),
                    ),
            )?;
            connection.execute(&sql, params_from_iter(values))?;
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
            connection.execute(&sql, params_from_iter(values))?;
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
            let row_id = self.vector_keys_get_by_search_hash(search_hash)?;
            let key = StoredVectorKey {
                row_id,
                is_inserted: inserted,
            };
            if key.is_inserted {
                let (sql, values) = build_query(
                    Query::insert()
                        .into_table(VectorIndex::Table)
                        .columns([VectorIndex::Rowid, VectorIndex::Vector])
                        .values([Expr::val(key.row_id), Expr::val(bytes)])
                        .map_err(|error| {
                            StorageError::new_with_cause(
                                StorageErrorKind::InternalError,
                                "invalid persisted storage value",
                                error,
                            )
                        })?,
                )?;
                connection.execute(&sql, params_from_iter(values))?;
            } else {
                let (sql, values) = build_query(
                    Query::select()
                        .expr(Expr::val(1))
                        .from(VectorIndex::Table)
                        .and_where(Expr::col(VectorIndex::Rowid).eq(key.row_id)),
                )?;
                let exists = connection
                    .query_row(&sql, params_from_iter(values), |_| Ok(()))
                    .optional()?;
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
            connection.execute(&sql, params_from_iter(values))?;
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
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(
        |error| {
            StorageError::new_with_cause(StorageErrorKind::InternalError, STORAGE_FAILURE, error)
        },
    )?;
    let current = decode_config_identity(meta_get_by_id(&tx, VectorSchema::Main)?)?;
    if current.as_ref() == Some(identity) {
        return Ok(VectorOutcome::Current(false));
    }
    if current.as_ref() != observed {
        return Ok(VectorOutcome::ConfigChanged);
    }
    tx.execute_batch(RESET_INDEX)?;
    let (sql, values) = build_query(Query::delete().from_table(VectorKeys::Table))?;
    tx.execute(&sql, params_from_iter(values))?;
    let (sql, values) = build_query(Query::delete().from_table(EmbedFailures::Table))?;
    tx.execute(&sql, params_from_iter(values))?;
    let (sql, values) = build_query(
        Query::insert()
            .into_table(Meta::Table)
            .columns([Meta::Id, Meta::ConfigIdentity])
            .values([Expr::val(1), Expr::val(identity.to_hex())])
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?
            .on_conflict(
                OnConflict::column(Meta::Id)
                    .update_column(Meta::ConfigIdentity)
                    .to_owned(),
            ),
    )?;
    tx.execute(&sql, params_from_iter(values))?;
    tx.commit()?;
    Ok(VectorOutcome::Current(true))
}

/// Returns the raw identity text from the selected database.
///
/// # Arguments
///
/// * `connection` — the connection holding the selected database.
/// * `schema` — which vector database to read, main or the attached `vec`.
///
/// # Errors
/// Reports database failures; interpreting malformed text belongs to the caller.
pub(super) fn meta_get_by_id(
    connection: &Connection,
    schema: VectorSchema,
) -> StorageResult<Option<String>> {
    let mut query = Query::select();
    query
        .column(Meta::ConfigIdentity)
        .and_where(Expr::col(Meta::Id).eq(1));
    match schema {
        VectorSchema::Main => query.from(Meta::Table),
        VectorSchema::Attached => query.from((Schemas::Vec, Meta::Table)),
    };
    let (sql, values) = build_query(&query)?;
    connection
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(StorageError::from)
}

/// Decodes an optional persisted configuration identity.
///
/// # Arguments
///
/// * `identity` — the optional raw identity text to decode as a SHA-256 hash.
///
/// # Errors
/// Reports malformed hashes with the established metadata diagnostic.
pub(super) fn decode_config_identity(identity: Option<String>) -> StorageResult<Option<Sha256>> {
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
        .optional()?;
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
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?
            .on_conflict(
                OnConflict::column(VectorKeys::SearchHash)
                    .do_nothing()
                    .to_owned(),
            ),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map(|changed| changed == 1)
        .map_err(StorageError::from)
}

/// Returns the decoded native vector joined through its current key.
///
/// # Arguments
///
/// * `connection` — the connection with the `vec` schema attached.
/// * `search_hash` — the search hash whose stored vector is read.
///
/// # Errors
/// Reports missing or malformed native rows and database failures.
pub(super) fn vector_keys_get_by_search_hash(
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
        .optional()?;
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

/// Decode native-endian f32 values without accepting empty, truncated or nonfinite payloads.
///
/// # Errors
/// Returns an internal storage error for invalid stored vector bytes.
pub(super) fn decode_vector(bytes: &[u8]) -> StorageResult<Vec<f32>> {
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

pub(super) fn vector_index_get_nearest_by_vector(
    connection: &Connection,
    query: &[f32],
    dimensions: usize,
    limit: NonZeroUsize,
) -> StorageResult<Vec<VectorMatch>> {
    let bytes = encode_vector(query, dimensions)?;
    let count = vector_keys_get(connection)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    vector_index_get_by_vector(connection, &bytes, count, limit)
}

fn vector_keys_get(connection: &Connection) -> StorageResult<i64> {
    let (sql, values) = build_query(
        Query::select()
            .expr(Expr::col(Asterisk).count())
            .from((Schemas::Vec, VectorKeys::Table)),
    )?;
    connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .map_err(StorageError::from)
}

fn vector_index_get_by_vector(
    connection: &Connection,
    bytes: &[u8],
    count: i64,
    limit: NonZeroUsize,
) -> StorageResult<Vec<VectorMatch>> {
    let mut statement = connection.prepare(
        "select n.uri, k.search_hash, v.distance
         from vec.vector_index(?1, ?2) v
         join vec.vector_keys k on k.row_id = v.rowid
         join nodes n on n.search_hash = k.search_hash
         where n.kind <> ?3
         order by v.distance, n.uri
         limit ?4",
    )?;
    let rows = statement.query_map(
        params![
            bytes,
            format!("{{K:{count}}}"),
            SOURCE_KIND,
            i64::try_from(limit.get()).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        },
    )?;
    rows.map(|row| {
        let (id, search_hash, distance) = row?;
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

fn encode_unsigned(integer: u64) -> StorageResult<i64> {
    i64::try_from(integer).map_err(|cause| {
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
        .optional()?;
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
        .optional()?;
    value
        .map(|(at, error)| {
            Ok(QueryError {
                at: decode_unsigned(at)?,
                error,
            })
        })
        .transpose()
}

fn decode_unsigned(integer: i64) -> StorageResult<u64> {
    u64::try_from(integer).map_err(|_| {
        StorageError::new(StorageErrorKind::InternalError, "invalid stored diagnostic")
    })
}

pub(super) fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
    connection: &Connection,
    max_attempts: u64,
    uri: Option<&KnowledgeId>,
) -> StorageResult<Option<EmbeddingWork>> {
    if max_attempts == 0 {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "attempt budget must be positive",
        ));
    }
    let mut query = Query::select();
    query
        .columns([Nodes::Uri, Nodes::SearchHash])
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
        .and_where(Expr::exists(build_vector_key_exists_query()).not())
        .and_where(Expr::exists(build_failure_threshold_query(max_attempts)).not())
        .order_by(Nodes::Uri, Order::Asc)
        .limit(1);
    if let Some(uri) = uri {
        query.and_where(Expr::col(Nodes::Uri).gt(uri.as_str()));
    }
    let (sql, values) = build_query(&query)?;
    let row = connection
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?;
    row.map(|(uri, hash)| {
        Ok(EmbeddingWork {
            id: KnowledgeId::parse(&uri).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
            search_hash: Sha256::parse(&hash).map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid persisted storage value",
                    error,
                )
            })?,
        })
    })
    .transpose()
}

fn build_failure_threshold_query(max_attempts: u64) -> SelectStatement {
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

fn build_current_nodes_query(hash: Expr) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Cache, Nodes::Table))
        .and_where(Expr::col((Nodes::Table, Nodes::Kind)).ne(SOURCE_KIND))
        .and_where(Expr::col((Nodes::Table, Nodes::SearchHash)).eq(hash))
        .to_owned()
}

/// Returns the count of distinct current non-source knowledge hashes in SQL.
///
/// # Arguments
///
/// * `connection` — the connection with the `cache` schema attached.
///
/// # Errors
/// Reports database failures or invalid aggregate values.
pub(super) fn nodes_get_by_kind(connection: &Connection) -> StorageResult<u64> {
    nodes_count_by_kind(connection, HashCoverage::All)
}

/// Returns the count of distinct current hashes with vector keys after verifying native rows.
///
/// # Arguments
///
/// * `connection` — the connection with the `cache` and `vec` schemas attached.
///
/// # Errors
/// Reports missing native rows, invalid counts and database failures.
pub(super) fn nodes_get_coverage_by_kind(connection: &Connection) -> StorageResult<u64> {
    verify_current_vectors(connection)?;
    nodes_count_by_kind(connection, HashCoverage::Stored)
}

fn verify_current_vectors(connection: &Connection) -> StorageResult<()> {
    if vector_key_orphan_get(connection)?.is_some() {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored vector data",
        ));
    }
    Ok(())
}

fn vector_key_orphan_get(connection: &Connection) -> StorageResult<Option<i64>> {
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
        .map_err(StorageError::from)
}

fn nodes_count_by_kind(connection: &Connection, coverage: HashCoverage) -> StorageResult<u64> {
    let mut query = Query::select();
    query
        .expr(Expr::col(Nodes::SearchHash).count_distinct())
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND));
    if matches!(coverage, HashCoverage::Stored) {
        query.and_where(Expr::exists(build_vector_key_exists_query()));
    }
    let (sql, values) = build_query(&query)?;
    let count: i64 = connection.query_row(&sql, params_from_iter(values), |row| row.get(0))?;
    u64::try_from(count).map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InternalError,
            "invalid persisted storage value",
            error,
        )
    })
}

/// Returns current uncovered failures, decoding every row before applying the threshold.
///
/// # Arguments
///
/// * `connection` — the connection with the `cache` and `vec` schemas attached.
/// * `max_attempts` — the attempt threshold at or above which failures are returned.
///
/// # Errors
/// Reports malformed hashes or attempts and database failures.
pub(super) fn embed_failures_get_by_kind_and_search_hash_and_attempts(
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
                    .and_where(Expr::exists(build_vector_key_exists_query()).not())
                    .to_owned(),
            ))
            .order_by(EmbedFailures::SearchHash, Order::Asc),
    )?;
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    // Decode every candidate before filtering: a low malformed count is still corruption.
    let mut exhausted = Vec::new();
    for row in rows {
        let failure = decode_embedding_failure(row?)?;
        if failure.attempts >= max_attempts {
            exhausted.push(failure);
        }
    }
    Ok(exhausted)
}

fn decode_embedding_failure(
    (hash, attempts, error): (String, i64, Option<String>),
) -> StorageResult<EmbeddingFailure> {
    let attempts = u64::try_from(attempts).map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InternalError,
            "invalid persisted storage value",
            error,
        )
    })?;
    if attempts == 0 {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored embedding attempts",
        ));
    }
    Ok(EmbeddingFailure {
        search_hash: Sha256::parse(&hash).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid persisted storage value",
                error,
            )
        })?,
        attempts,
        error,
    })
}

fn build_vector_key_exists_query() -> SelectStatement {
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
#[path = "vector_test.rs"]
mod tests;
