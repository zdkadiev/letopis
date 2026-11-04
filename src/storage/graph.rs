use super::{
    connection::map_sqlite,
    edges::{demote_incoming_edges, verify_no_outgoing},
    query::{Nodes, TextIndex, build_query},
};
use crate::{
    domain::{EdgeTarget, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid},
    service::{
        KnowledgeMetadata, NodeMetadata, SourceMetadata, StorageError, StorageErrorKind,
        StorageResult, StoredNode,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query, Value};

pub(super) const SOURCE_KIND: &str = "source";
pub(super) const VERTEX_KIND: &str = "vertex";
pub(super) const METAVERTEX_KIND: &str = "metavertex";
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

pub(super) fn upsert_node(db: &Connection, node: &NodeMetadata) -> StorageResult<i64> {
    let query = Query::insert()
        .into_table(Nodes::Table)
        .columns(NODE_COLUMNS)
        .values(build_node_values(node)?.map(Expr::val))
        .map_err(map_invalid_value)?
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
    let start = lines.map(|r| check_unsigned(r.get_start())).transpose()?;
    let end = lines.map(|r| check_unsigned(r.get_end())).transpose()?;
    Ok([
        uri.into(),
        kind.into(),
        hash.into(),
        check_unsigned(created)?.into(),
        snapshot.into(),
        start.into(),
        end.into(),
        search.into(),
    ])
}

pub(super) fn check_unsigned(value: u64) -> StorageResult<i64> {
    i64::try_from(value).map_err(|e| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "storage integer out of range",
            e,
        )
    })
}

pub(super) fn get_node_by_id(
    db: &Connection,
    id: &EdgeTarget,
) -> StorageResult<Option<StoredNode>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::Id)
            .columns(NODE_COLUMNS)
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Uri).eq(id.as_str())),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| Ok(decode_node(row)))
        .optional()
        .map_err(map_sqlite)?
        .transpose()
}

pub(super) fn get_nodes(db: &Connection) -> StorageResult<Vec<StoredNode>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::Id)
            .columns(NODE_COLUMNS)
            .from(Nodes::Table)
            .order_by(Nodes::Uri, Order::Asc),
    )?;
    let mut statement = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(map_sqlite)?;
    let mut nodes = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        nodes.push(decode_node(row)?);
    }
    Ok(nodes)
}

fn decode_node(row: &Row<'_>) -> StorageResult<StoredNode> {
    let uri: String = row.get(1).map_err(map_sqlite)?;
    let kind: String = row.get(2).map_err(map_sqlite)?;
    let hash: Option<String> = row.get(3).map_err(map_sqlite)?;
    let created_at = get_unsigned(row.get(4).map_err(map_sqlite)?)?;
    let snapshot: Option<String> = row.get(5).map_err(map_sqlite)?;
    let lines = decode_lines(
        row.get(6).map_err(map_sqlite)?,
        row.get(7).map_err(map_sqlite)?,
    )?;
    let search: Option<String> = row.get(8).map_err(map_sqlite)?;
    let hash = hash
        .as_deref()
        .map(Sha256::parse)
        .transpose()
        .map_err(map_invalid_value)?;
    let metadata = if kind == SOURCE_KIND {
        if snapshot.is_some() || search.is_some() {
            return Err(build_invalid_metadata_error());
        }
        NodeMetadata::Source(SourceMetadata {
            uri: SourceUri::parse(&uri).map_err(map_invalid_value)?,
            hash,
            created_at,
            lines,
        })
    } else {
        let kind = match kind.as_str() {
            VERTEX_KIND => ElementKind::Vertex,
            METAVERTEX_KIND => ElementKind::Metavertex,
            _ => return Err(build_invalid_metadata_error()),
        };
        NodeMetadata::Knowledge(KnowledgeMetadata {
            id: KnowledgeId::parse(&uri).map_err(map_invalid_value)?,
            uri: KnowledgeId::parse(&uri).map_err(map_invalid_value)?,
            kind,
            hash: hash.ok_or_else(build_invalid_metadata_error)?,
            created_at,
            source_snapshot: snapshot
                .as_deref()
                .map(TreeOid::parse)
                .transpose()
                .map_err(map_invalid_value)?,
            lines: lines.ok_or_else(build_invalid_metadata_error)?,
            search_hash: Sha256::parse(&search.ok_or_else(build_invalid_metadata_error)?)
                .map_err(map_invalid_value)?,
        })
    };
    Ok(StoredNode {
        row_id: row.get(0).map_err(map_sqlite)?,
        metadata,
    })
}

fn decode_lines(start: Option<i64>, end: Option<i64>) -> StorageResult<Option<LineRange>> {
    match (start, end) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) => Ok(Some(
            LineRange::new(get_unsigned(a)?, get_unsigned(b)?).map_err(map_invalid_value)?,
        )),
        _ => Err(build_invalid_metadata_error()),
    }
}

pub(super) fn get_unsigned(value: i64) -> StorageResult<u64> {
    u64::try_from(value).map_err(map_invalid_value)
}

fn build_invalid_metadata_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "invalid persisted metadata",
    )
}

pub(super) fn map_invalid_value(
    error: impl std::error::Error + Send + Sync + 'static,
) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        "invalid persisted storage value",
        error,
    )
}

/// Delete an existing node after checking owned declarations and preserving incoming links.
///
/// The caller owns the transaction. Missing nodes are a no-op; successful deletion
/// demotes incoming declarations before removing the node and its FTS projection.
///
/// # Errors
/// Rejects nodes with outgoing declarations and reports mutation failures.
pub(super) fn delete_node(db: &Connection, id: &EdgeTarget) -> StorageResult<()> {
    let Some(node_id) = get_node_id_by_uri(db, id.as_str())? else {
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

/// Get the internal key for an existing public address without decoding metadata.
///
/// # Errors
/// Reports query or database failures.
pub(super) fn get_node_id_by_uri(db: &Connection, uri: &str) -> StorageResult<Option<i64>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::Id)
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Uri).eq(uri)),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(map_sqlite)
}

pub(super) fn map_mutation(error: rusqlite::Error) -> StorageError {
    if matches!(&error, rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::ConstraintViolation)
    {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "invalid storage mutation",
            error,
        )
    } else {
        map_sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_node, delete_node};
    use crate::{
        domain::{EdgeTarget, KnowledgeId},
        service::StorageErrorKind,
        storage::schema::bootstrap_cache,
    };
    use rusqlite::{Connection, params};

    #[test]
    fn deletion_preflight_preserves_incoming_declarations_and_fts_before_rejection() {
        // Given: a target has an incoming edge, pending outgoing declaration and searchable text.
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        db.execute_batch(
            "insert into nodes(id,uri,kind,hash,created_at,line_start,line_end,search_hash)
            values(1,'a.md','vertex','unused',1,1,1,'unused'),
                  (2,'b.md','vertex','unused',1,1,1,'unused');
            insert into edges values(1,2,'supports');
            insert into unresolved_edges values(2,'missing.md','supports');
            insert into text_index(rowid,text) values(2,'targettoken');",
        )
        .unwrap();
        let transaction = db.transaction().unwrap();
        // When: deletion is rejected inside the still-open transaction.
        let rejected = delete_node(
            &transaction,
            &EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
        );
        // Then: rejection preceded demotion and FTS mutation, without relying on rollback.
        assert_eq!(
            rejected.unwrap_err().get_kind(),
            StorageErrorKind::InvalidRequest
        );
        assert_eq!(
            transaction
                .query_row(
                    "select count(*) from edges where source_id=1 and target_id=2",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            transaction
                .query_row("select count(*) from unresolved_edges", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            transaction
                .query_row("select count(*) from text_index where rowid=2", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
    }

    #[test]
    fn rejects_each_inconsistent_persisted_source_field() {
        let connection = Connection::open_in_memory().unwrap();
        for (snapshot, search) in [
            (Some("unexpected snapshot"), None),
            (None, Some("unexpected search hash")),
        ] {
            // Given: each row violates exactly one source-specific invariant.
            let mut statement = connection
                .prepare("select 1, 'code.rs', 'source', null, 0, ?1, null, null, ?2")
                .unwrap();
            let mut rows = statement.query(params![snapshot, search]).unwrap();
            let row = rows.next().unwrap().unwrap();
            // When: the decoder encounters the invalid source-only field.
            let error = decode_node(row).unwrap_err();
            // Then: corrupt persisted source metadata is rejected.
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        }
    }
}
