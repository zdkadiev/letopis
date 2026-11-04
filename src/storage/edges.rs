use super::{
    connection::map_sqlite,
    graph::{get_node_id_by_uri, map_invalid_value, map_mutation},
    query::{Edges, Nodes, UnresolvedEdges, build_query},
};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri},
    service::{StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement, UnionType};

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

/// Insert one logical declaration, retaining absent targets without placeholder nodes.
///
/// # Errors
/// Rejects missing sources and duplicates in either representation as invalid requests;
/// reports database failures. The caller owns the surrounding transaction.
pub(super) fn insert_edge(db: &Connection, edge: &Edge) -> StorageResult<()> {
    let source_id = get_node_id_by_uri(db, edge.get_source().as_str())?
        .ok_or_else(build_invalid_declaration_error)?;
    let target_uri = edge.get_target().as_str();
    let target_id = get_node_id_by_uri(db, target_uri)?;
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
            .map_err(map_invalid_value)?;
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
            .map_err(map_invalid_value)?;
    }
    let (sql, values) = build_query(&query)?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}

/// Get resolved and unresolved declarations ordered by source URI, target URI and type.
///
/// # Errors
/// Reports query failures or invalid persisted domain values.
pub(super) fn get_edges_by_source_id(
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
    let mut statement = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(map_sqlite)?;
    let mut edges = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        edges.push(decode_edge(row)?);
    }
    Ok(edges)
}

/// Remove both declaration representations owned by a source; unknown owners are a no-op.
///
/// # Errors
/// Reports query or database failures inside the caller's transaction.
pub(super) fn delete_outgoing(db: &Connection, source: &KnowledgeId) -> StorageResult<()> {
    let Some(source_id) = get_node_id_by_uri(db, source.as_str())? else {
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

/// Reject node deletion while any owned declaration remains, before changing projections.
///
/// # Errors
/// Returns an invalid request for remaining declarations or reports database failures.
pub(super) fn verify_no_outgoing(db: &Connection, source_id: i64) -> StorageResult<()> {
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
    let has_outgoing: bool = db
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .map_err(map_sqlite)?;
    if has_outgoing {
        return Err(build_invalid_declaration_error());
    }
    Ok(())
}

/// Preserve incoming declarations before deleting their target's restrictive numeric FK.
///
/// # Errors
/// Reports mutation failures; the caller must roll back the transaction on error.
pub(super) fn demote_incoming_edges(
    db: &Connection,
    target_id: i64,
    target_uri: &str,
) -> StorageResult<()> {
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
            .map_err(map_invalid_value)?,
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

/// Promote all declarations whose targets exist, without collecting graph rows in Rust.
///
/// # Errors
/// Reports mutation failures; the caller must roll back rather than publish a revision.
pub(super) fn resolve_edges(db: &Connection) -> StorageResult<()> {
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
            .map_err(map_invalid_value)?,
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
    let duplicate: bool = db
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .map_err(map_sqlite)?;
    if duplicate {
        return Err(build_invalid_declaration_error());
    }
    Ok(())
}

fn build_invalid_declaration_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::InvalidRequest,
        "invalid storage declaration",
    )
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
    let source: String = row.get(0).map_err(map_sqlite)?;
    let target: String = row.get(1).map_err(map_sqlite)?;
    let edge_type: String = row.get(2).map_err(map_sqlite)?;
    let target = if edge_type == "external" {
        EdgeTarget::Source(SourceUri::parse(&target).map_err(map_invalid_value)?)
    } else {
        EdgeTarget::Knowledge(KnowledgeId::parse(&target).map_err(map_invalid_value)?)
    };
    Edge::new(
        KnowledgeId::parse(&source).map_err(map_invalid_value)?,
        target,
        EdgeType::parse(&edge_type).map_err(map_invalid_value)?,
    )
    .map_err(map_invalid_value)
}
