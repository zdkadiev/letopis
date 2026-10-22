use super::connection::map_sqlite;
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    service::{
        KnowledgeMetadata, NodeMetadata, SourceMetadata, StorageError, StorageErrorKind,
        StorageResult, StoredNode,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params};

pub(super) fn map_invalid_value(
    error: impl std::error::Error + Send + Sync + 'static,
) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        "invalid persisted storage value",
        error,
    )
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
pub(super) fn read_unsigned(value: i64) -> StorageResult<u64> {
    u64::try_from(value).map_err(map_invalid_value)
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
pub(super) fn upsert_node(db: &Connection, node: &NodeMetadata) -> StorageResult<i64> {
    let (id, kind, uri, hash, created, snapshot, lines, search) = match node {
        NodeMetadata::Knowledge(n) => (
            n.id.as_str(),
            match n.kind {
                ElementKind::Vertex => "vertex",
                ElementKind::Metavertex => "metavertex",
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
            "source",
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
    db.query_row(
        "INSERT INTO nodes(id,kind,uri,hash,created_at,source_snapshot,line_start,line_end,search_hash)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,uri=excluded.uri,
         hash=excluded.hash,created_at=excluded.created_at,source_snapshot=excluded.source_snapshot,
         line_start=excluded.line_start,line_end=excluded.line_end,search_hash=excluded.search_hash
         RETURNING row_id",
        params![id,kind,uri,hash,check_unsigned(created)?,snapshot,start,end,search],
        |row| row.get(0),
    ).map_err(map_mutation)
}
pub(super) fn read_node(db: &Connection, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
    db.query_row(
        "SELECT row_id,id,kind,uri,hash,created_at,source_snapshot,line_start,line_end,search_hash
         FROM nodes WHERE id=?1",
        [id.as_str()],
        |row| Ok(decode_node(row)),
    )
    .optional()
    .map_err(map_sqlite)?
    .transpose()
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
    let metadata = if kind == "source" {
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
            "vertex" => ElementKind::Vertex,
            "metavertex" => ElementKind::Metavertex,
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
fn build_invalid_metadata_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "invalid persisted metadata",
    )
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
pub(super) fn insert_edge(db: &Connection, edge: &Edge) -> StorageResult<()> {
    db.execute(
        "INSERT INTO edges(source_id,target_id,type) VALUES(?1,?2,?3)",
        params![
            edge.get_source().as_str(),
            edge.get_target().as_str(),
            edge.get_edge_type().as_str()
        ],
    )
    .map_err(map_mutation)?;
    Ok(())
}
pub(super) fn list_edges(
    db: &Connection,
    source: Option<&KnowledgeId>,
) -> StorageResult<Vec<Edge>> {
    let mut stmt = db
        .prepare(
            "SELECT source_id,target_id,type FROM edges
         WHERE ?1 IS NULL OR source_id=?1 ORDER BY source_id,target_id,type",
        )
        .map_err(map_sqlite)?;
    let mut rows = stmt
        .query([source.map(KnowledgeId::as_str)])
        .map_err(map_sqlite)?;
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
pub(super) fn delete_outgoing(db: &Connection, source: &KnowledgeId) -> StorageResult<()> {
    db.execute("DELETE FROM edges WHERE source_id=?1", [source.as_str()])
        .map_err(map_mutation)?;
    Ok(())
}
pub(super) fn delete_node(db: &Connection, id: &EdgeTarget) -> StorageResult<()> {
    db.execute(
        "DELETE FROM text_index WHERE rowid IN (SELECT row_id FROM nodes WHERE id=?1)",
        [id.as_str()],
    )
    .map_err(map_mutation)?;
    db.execute("DELETE FROM nodes WHERE id=?1", [id.as_str()])
        .map_err(map_mutation)?;
    Ok(())
}
