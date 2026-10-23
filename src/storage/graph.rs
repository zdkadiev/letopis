use super::{
    connection::map_sqlite,
    query::{Edges, Nodes, TextIndex, build_query},
};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    service::{
        KnowledgeMetadata, NodeMetadata, SourceMetadata, StorageError, StorageErrorKind,
        StorageResult, StoredNode,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query, Value};

pub(super) const SOURCE_KIND: &str = "source";
const VERTEX_KIND: &str = "vertex";
const METAVERTEX_KIND: &str = "metavertex";
const NODE_COLUMNS: [Nodes; 9] = [
    Nodes::Id,
    Nodes::Kind,
    Nodes::Uri,
    Nodes::Hash,
    Nodes::CreatedAt,
    Nodes::SourceSnapshot,
    Nodes::LineStart,
    Nodes::LineEnd,
    Nodes::SearchHash,
];
const EDGE_COLUMNS: [Edges; 3] = [Edges::SourceId, Edges::TargetId, Edges::Type];

pub(super) fn upsert_node(db: &Connection, node: &NodeMetadata) -> StorageResult<i64> {
    let query = Query::insert()
        .into_table(Nodes::Table)
        .columns(NODE_COLUMNS)
        .values(build_node_values(node)?.map(Expr::val))
        .map_err(map_invalid_value)?
        .on_conflict(
            OnConflict::column(Nodes::Id)
                .update_columns(NODE_COLUMNS.into_iter().skip(1))
                .to_owned(),
        )
        .returning_col(Nodes::RowId)
        .to_owned();
    let (sql, values) = build_query(&query)?;
    db.query_row(&sql, params_from_iter(values), |row| row.get(0))
        .map_err(map_mutation)
}

fn build_node_values(node: &NodeMetadata) -> StorageResult<[Value; 9]> {
    let (id, kind, uri, hash, created, snapshot, lines, search) = match node {
        NodeMetadata::Knowledge(n) => (
            n.id.as_str(),
            match n.kind {
                ElementKind::Vertex => VERTEX_KIND,
                ElementKind::Metavertex => METAVERTEX_KIND,
            },
            n.uri.as_str(),
            Some(n.hash.to_hex()),
            n.created_at,
            n.source_snapshot.as_ref().map(TreeOid::as_str),
            Some(n.lines),
            Some(n.search_hash.to_hex()),
        ),
        NodeMetadata::Source(n) => (
            n.uri.as_str(),
            SOURCE_KIND,
            n.uri.as_str(),
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
        id.into(),
        kind.into(),
        uri.into(),
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

pub(super) fn read_node(db: &Connection, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::RowId)
            .columns(NODE_COLUMNS)
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Id).eq(id.as_str())),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| Ok(decode_node(row)))
        .optional()
        .map_err(map_sqlite)?
        .transpose()
}

pub(super) fn list_nodes(db: &Connection) -> StorageResult<Vec<StoredNode>> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::RowId)
            .columns(NODE_COLUMNS)
            .from(Nodes::Table)
            .order_by(Nodes::Id, Order::Asc),
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
    let id: String = row.get(1).map_err(map_sqlite)?;
    let kind: String = row.get(2).map_err(map_sqlite)?;
    let uri: String = row.get(3).map_err(map_sqlite)?;
    let hash: Option<String> = row.get(4).map_err(map_sqlite)?;
    let created_at = read_unsigned(row.get(5).map_err(map_sqlite)?)?;
    let snapshot: Option<String> = row.get(6).map_err(map_sqlite)?;
    let lines = decode_lines(
        row.get(7).map_err(map_sqlite)?,
        row.get(8).map_err(map_sqlite)?,
    )?;
    let search: Option<String> = row.get(9).map_err(map_sqlite)?;
    let hash = hash
        .as_deref()
        .map(Sha256::parse)
        .transpose()
        .map_err(map_invalid_value)?;
    let metadata = if kind == SOURCE_KIND {
        if id != uri || snapshot.is_some() || search.is_some() {
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
            id: KnowledgeId::parse(&id).map_err(map_invalid_value)?,
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
            LineRange::new(read_unsigned(a)?, read_unsigned(b)?).map_err(map_invalid_value)?,
        )),
        _ => Err(build_invalid_metadata_error()),
    }
}

pub(super) fn read_unsigned(value: i64) -> StorageResult<u64> {
    u64::try_from(value).map_err(map_invalid_value)
}

fn build_invalid_metadata_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "invalid persisted metadata",
    )
}

pub(super) fn insert_edge(db: &Connection, edge: &Edge) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::insert()
            .into_table(Edges::Table)
            .columns(EDGE_COLUMNS)
            .values(
                [
                    edge.get_source().as_str(),
                    edge.get_target().as_str(),
                    edge.get_edge_type().as_str(),
                ]
                .map(Expr::val),
            )
            .map_err(map_invalid_value)?,
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

pub(super) fn list_edges(
    db: &Connection,
    source: Option<&KnowledgeId>,
) -> StorageResult<Vec<Edge>> {
    let mut query = Query::select();
    query.columns(EDGE_COLUMNS).from(Edges::Table);
    if let Some(source) = source {
        query.and_where(Expr::col(Edges::SourceId).eq(source.as_str()));
    }
    for column in EDGE_COLUMNS {
        query.order_by(column, Order::Asc);
    }
    let (sql, values) = build_query(&query)?;
    let mut stmt = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = stmt.query(params_from_iter(values)).map_err(map_sqlite)?;
    let mut edges = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        let source: String = row.get(0).map_err(map_sqlite)?;
        let target: String = row.get(1).map_err(map_sqlite)?;
        let kind: String = row.get(2).map_err(map_sqlite)?;
        let target = if kind == "external" {
            EdgeTarget::Source(SourceUri::parse(&target).map_err(map_invalid_value)?)
        } else {
            EdgeTarget::Knowledge(KnowledgeId::parse(&target).map_err(map_invalid_value)?)
        };
        edges.push(
            Edge::new(
                KnowledgeId::parse(&source).map_err(map_invalid_value)?,
                target,
                EdgeType::parse(&kind).map_err(map_invalid_value)?,
            )
            .map_err(map_invalid_value)?,
        );
    }
    Ok(edges)
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

pub(super) fn delete_outgoing(db: &Connection, source: &KnowledgeId) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Edges::Table)
            .and_where(Expr::col(Edges::SourceId).eq(source.as_str())),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

pub(super) fn delete_node(db: &Connection, id: &EdgeTarget) -> StorageResult<()> {
    let node = Query::select()
        .column(Nodes::RowId)
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = build_query(
        Query::delete()
            .from_table(TextIndex::Table)
            .and_where(Expr::col(TextIndex::Rowid).in_subquery(node)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Nodes::Table)
            .and_where(Expr::col(Nodes::Id).eq(id.as_str())),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
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
    use super::decode_node;
    use crate::service::StorageErrorKind;
    use rusqlite::{Connection, params};

    #[test]
    fn rejects_each_inconsistent_persisted_source_field() {
        let connection = Connection::open_in_memory().unwrap();
        for (id, snapshot, search) in [
            ("different.rs", None, None),
            ("code.rs", Some("unexpected snapshot"), None),
            ("code.rs", None, Some("unexpected search hash")),
        ] {
            // Arrange: each row violates exactly one source-specific invariant.
            let mut statement = connection
                .prepare("SELECT 1, ?1, 'source', 'code.rs', NULL, 0, ?2, NULL, NULL, ?3")
                .unwrap();
            let mut rows = statement.query(params![id, snapshot, search]).unwrap();
            let row = rows.next().unwrap().unwrap();
            // Act
            let error = decode_node(row).unwrap_err();
            // Assert
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        }
    }
}
