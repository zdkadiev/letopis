use super::{
    connection::map_sqlite,
    graph::{SOURCE_KIND, map_invalid_value},
    query::{EmbedFailures, Meta, QueryErrorRow, Schemas, VectorIndex, VectorKeys, build_query},
};
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{
        EmbeddingFailure, QueryError, StorageError, StorageErrorKind, StorageResult,
        StoredVectorKey, VectorMatch, VectorOutcome,
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use sea_query::{Asterisk, Expr, ExprTrait, JoinType, OnConflict, Query};
use std::num::NonZeroUsize;

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

pub(super) fn reset_vectors(
    conn: &mut Connection,
    observed: Option<&Sha256>,
    identity: &Sha256,
) -> StorageResult<VectorOutcome<bool>> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sqlite)?;
    let current = get_config_identity(&tx, VectorSchema::Main)?;
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

pub(super) fn get_config_identity(
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

pub(super) fn matches_identity(connection: &Connection, expected: &Sha256) -> StorageResult<bool> {
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

pub(super) fn store_vector(
    connection: &Connection,
    search_hash: &Sha256,
    vector: &[f32],
    dimensions: usize,
) -> StorageResult<StoredVectorKey> {
    let bytes = encode_vector(vector, dimensions)?;
    validate_dimensions(connection, dimensions)?;
    let key = get_or_insert_vector_key(connection, search_hash)?;
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
            return Err(fail_invalid_stored_vector());
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
}

fn validate_dimensions(connection: &Connection, dimensions: usize) -> StorageResult<()> {
    let current = get_vector_dimensions(connection, VectorSchema::Main)?;
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

pub(super) fn get_vector_dimensions(
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
            let size = usize::try_from(value).map_err(|_| fail_invalid_stored_vector())?;
            if !(NATIVE_DIMENSION_MIN..=NATIVE_DIMENSION_MAX).contains(&size) {
                return Err(fail_invalid_stored_vector());
            }
            NonZeroUsize::new(size).ok_or_else(fail_invalid_stored_vector)
        })
        .transpose()
}

fn get_or_insert_vector_key(
    connection: &Connection,
    search_hash: &Sha256,
) -> StorageResult<StoredVectorKey> {
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
    let inserted = connection
        .execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?
        == 1;
    let (sql, values) = build_query(
        Query::select()
            .column(VectorKeys::RowId)
            .from(VectorKeys::Table)
            .and_where(Expr::col(VectorKeys::SearchHash).eq(search_hash.to_hex())),
    )?;
    let row_id = connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .map_err(map_sqlite)?;
    Ok(StoredVectorKey { row_id, inserted })
}

pub(super) fn get_vector_by_search_hash(
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
        .map(|bytes| decode_vector(&bytes.ok_or_else(fail_invalid_stored_vector)?))
        .transpose()
}

fn decode_vector(bytes: &[u8]) -> StorageResult<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(fail_invalid_stored_vector());
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err(fail_invalid_stored_vector());
    }
    Ok(values)
}

pub(super) fn get_nearest_vectors(
    connection: &Connection,
    query: &[f32],
    dimensions: usize,
) -> StorageResult<Vec<VectorMatch>> {
    let bytes = encode_vector(query, dimensions)?;
    let (sql, values) = build_query(
        Query::select()
            .expr(Expr::col(Asterisk).count())
            .from((Schemas::Vec, VectorKeys::Table)),
    )?;
    let count: i64 = connection
        .query_row(&sql, params_from_iter(values), |r| r.get(0))
        .map_err(map_sqlite)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "select n.uri, k.search_hash, v.distance
         from vec.vector_index(?1, ?2) v
         join vec.vector_keys k on k.row_id = v.rowid
         join nodes n on n.search_hash = k.search_hash
         where n.kind <> ?3
         order by v.distance, n.uri",
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
            id: KnowledgeId::parse(&id).map_err(|_| fail_invalid_stored_vector())?,
            search_hash: Sha256::parse(&search_hash).map_err(|_| fail_invalid_stored_vector())?,
            distance,
        })
    })
    .collect()
}

pub(super) fn encode_vector(vector: &[f32], dimensions: usize) -> StorageResult<Vec<u8>> {
    if vector.is_empty() || vector.len() != dimensions || vector.iter().any(|v| !v.is_finite()) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector values or dimensions",
        ));
    }
    Ok(vector.iter().flat_map(|v| v.to_ne_bytes()).collect())
}

/// Construct the shared integrity error for unusable stored vector state.
pub(super) fn fail_invalid_stored_vector() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "invalid stored vector data",
    )
}

pub(super) fn reserve_attempt(
    connection: &Connection,
    search_hash: &Sha256,
    max_attempts: u64,
) -> StorageResult<bool> {
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
}

pub(super) fn record_failure(
    connection: &Connection,
    search_hash: &Sha256,
    error: &str,
) -> StorageResult<bool> {
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
}

pub(super) fn set_query_error(connection: &Connection, at: u64, error: &str) -> StorageResult<()> {
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

pub(super) fn clear_query_error(connection: &Connection) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::delete()
            .from_table(QueryErrorRow::Table)
            .and_where(Expr::col(QueryErrorRow::Id).eq(1)),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    Ok(())
}

pub(super) fn get_failure_by_search_hash(
    connection: &Connection,
    search_hash: &Sha256,
) -> StorageResult<Option<EmbeddingFailure>> {
    let (sql, values) = build_query(
        Query::select()
            .columns([EmbedFailures::Attempts, EmbedFailures::Error])
            .from((Schemas::Vec, EmbedFailures::Table))
            .and_where(Expr::col(EmbedFailures::SearchHash).eq(search_hash.to_hex())),
    )?;
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
                return Err(fail_invalid_diagnostic());
            }
            Ok(EmbeddingFailure {
                search_hash: *search_hash,
                attempts,
                error,
            })
        })
        .transpose()
}

pub(super) fn get_query_error(connection: &Connection) -> StorageResult<Option<QueryError>> {
    let (sql, values) = build_query(
        Query::select()
            .columns([QueryErrorRow::At, QueryErrorRow::Error])
            .from((Schemas::Vec, QueryErrorRow::Table))
            .and_where(Expr::col(QueryErrorRow::Id).eq(1)),
    )?;
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
    u64::try_from(value).map_err(|_| fail_invalid_diagnostic())
}

fn fail_invalid_diagnostic() -> StorageError {
    StorageError::new(StorageErrorKind::InternalError, "invalid stored diagnostic")
}

#[cfg(test)]
mod tests {
    use super::{
        VectorSchema, decode_vector, get_config_identity, get_vector_dimensions, reset_vectors,
        store_vector,
    };
    use crate::{
        domain::Sha256,
        service::StorageErrorKind,
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
        for (connection, identity, vector) in [
            (&mut main, &main_identity, vec![1.0, 0.0]),
            (&mut attached, &attached_identity, vec![1.0, 0.0, 0.0]),
        ] {
            bootstrap_vectors(connection).unwrap();
            reset_vectors(connection, None, identity).unwrap();
            store_vector(connection, identity, &vector, vector.len()).unwrap();
        }
        main.execute("attach database ?1 as vec", [path.to_str().unwrap()])
            .unwrap();

        // When: metadata is retrieved through each explicit schema selector.
        let main_id = get_config_identity(&main, VectorSchema::Main).unwrap();
        let attached_id = get_config_identity(&main, VectorSchema::Attached).unwrap();
        let main_dimensions = get_vector_dimensions(&main, VectorSchema::Main).unwrap();
        let attached_dimensions = get_vector_dimensions(&main, VectorSchema::Attached).unwrap();

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
