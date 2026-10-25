use super::{
    connection::map_sqlite,
    graph::{METAVERTEX_KIND, SOURCE_KIND, VERTEX_KIND, map_invalid_value},
    query::{Edges, Nodes, build_query},
};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri, TreeOid},
    service::{
        ElementAddress, GraphNeighbor, GraphNodeKey, GraphNodeRef, NeighborDirection,
        NeighborPageRequest, StorageError, StorageErrorKind, StorageResult, TraversalMode,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement};
use std::collections::BTreeSet;

const NEIGHBOR_PAGE_QUANTUM: usize = 32;
#[derive(Clone, Copy, Iden)]
enum Endpoint {
    Source,
    Target,
}

pub(super) fn resolve_node(
    db: &Connection,
    id: &ElementAddress,
) -> StorageResult<Option<GraphNodeRef>> {
    let (sql, values) = build_query(
        Query::select()
            .columns([Nodes::RowId, Nodes::Id, Nodes::Kind])
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Id).eq(id.as_str())),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| {
        Ok(decode_node_ref(row))
    })
    .optional()
    .map_err(map_sqlite)?
    .transpose()
}

pub(super) fn read_neighbor_page(
    db: &Connection,
    request: &NeighborPageRequest<'_>,
) -> StorageResult<Vec<GraphNeighbor>> {
    if request.limit.get() > NEIGHBOR_PAGE_QUANTUM {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid neighbor page limit",
        ));
    }
    let (sql, values) = build_query(&build_neighbor_query(request))?;
    let mut statement = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(map_sqlite)?;
    let mut neighbors = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        neighbors.push(decode_neighbor(row)?);
    }
    Ok(neighbors)
}

pub(super) fn list_source_snapshots(db: &Connection) -> StorageResult<BTreeSet<TreeOid>> {
    let (sql, values) = build_query(&build_source_snapshots_query())?;
    let mut statement = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(map_sqlite)?;
    let mut snapshots = BTreeSet::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        let raw: String = row.get(0).map_err(map_sqlite)?;
        snapshots.insert(TreeOid::parse(&raw).map_err(map_invalid_value)?);
    }
    Ok(snapshots)
}

fn build_neighbor_query(request: &NeighborPageRequest<'_>) -> SelectStatement {
    let (anchor, neighbor, neighbor_id) = match request.direction {
        NeighborDirection::Outgoing => (Endpoint::Source, Endpoint::Target, Edges::TargetId),
        NeighborDirection::Incoming => (Endpoint::Target, Endpoint::Source, Edges::SourceId),
    };
    let mut query = Query::select();
    query
        .columns([Nodes::RowId, Nodes::Id, Nodes::Kind].map(|col| (neighbor, col)))
        .columns([Edges::SourceId, Edges::TargetId, Edges::Type].map(|col| (Edges::Table, col)))
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
        .and_where(Expr::col((anchor, Nodes::RowId)).eq(request.node.get_value()));
    apply_filters(&mut query, request, neighbor_id);
    query
        .order_by((Edges::Table, Edges::Type), Order::Asc)
        .order_by((Edges::Table, neighbor_id), Order::Asc)
        .limit(request.limit.get() as u64);
    query
}

fn apply_filters(query: &mut SelectStatement, request: &NeighborPageRequest<'_>, neighbor: Edges) {
    let kind = Expr::col((Edges::Table, Edges::Type));
    match request.mode {
        TraversalMode::Relations => {
            query.and_where(kind.clone().ne("includes"));
        }
        TraversalMode::Containment => {
            query.and_where(kind.clone().eq("includes"));
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
            Expr::tuple([kind, Expr::col((Edges::Table, neighbor))]).gt(Expr::tuple([
                Expr::val(cursor.edge_type.as_str()),
                Expr::val(cursor.neighbor.as_str()),
            ])),
        );
    }
}

fn build_source_snapshots_query() -> SelectStatement {
    Query::select()
        .distinct()
        .column(Nodes::SourceSnapshot)
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::SourceSnapshot).is_not_null())
        .order_by(Nodes::SourceSnapshot, Order::Asc)
        .to_owned()
}

fn decode_neighbor(row: &Row<'_>) -> StorageResult<GraphNeighbor> {
    let source: String = row.get(3).map_err(map_sqlite)?;
    let target: String = row.get(4).map_err(map_sqlite)?;
    let edge_type: String = row.get(5).map_err(map_sqlite)?;
    let target_kind: String = row.get(6).map_err(map_sqlite)?;
    Ok(GraphNeighbor {
        node: decode_node_ref(row)?,
        edge: Edge::new(
            KnowledgeId::parse(&source).map_err(map_invalid_value)?,
            decode_identity(&target, &target_kind)?,
            EdgeType::parse(&edge_type).map_err(map_invalid_value)?,
        )
        .map_err(map_invalid_value)?,
    })
}

fn decode_node_ref(row: &Row<'_>) -> StorageResult<GraphNodeRef> {
    let id: String = row.get(1).map_err(map_sqlite)?;
    let kind: String = row.get(2).map_err(map_sqlite)?;
    Ok(GraphNodeRef {
        key: GraphNodeKey::new(row.get(0).map_err(map_sqlite)?),
        element: decode_identity(&id, &kind)?,
    })
}

fn decode_identity(id: &str, kind: &str) -> StorageResult<EdgeTarget> {
    match kind {
        SOURCE_KIND => SourceUri::parse(id)
            .map(EdgeTarget::Source)
            .map_err(map_invalid_value),
        VERTEX_KIND | METAVERTEX_KIND => KnowledgeId::parse(id)
            .map(EdgeTarget::Knowledge)
            .map_err(map_invalid_value),
        _ => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored node kind",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_neighbor_query, build_source_snapshots_query, list_source_snapshots};
    use crate::{
        domain::{EdgeType, TreeOid},
        service::{
            ElementAddress, GraphNodeKey, NeighborCursor, NeighborDirection, NeighborPageRequest,
            StorageErrorKind, TraversalMode,
        },
        storage::{query::build_query, schema::bootstrap_cache},
    };
    use rusqlite::{Connection, params_from_iter};
    use sea_query::SelectStatement;
    use std::{collections::BTreeSet, num::NonZeroUsize};

    #[test]
    fn pages_use_directional_indexes_without_temporary_order_sorts() {
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        let filters = [
            BTreeSet::new(),
            BTreeSet::from([EdgeType::parse("supports").unwrap()]),
            BTreeSet::from([
                EdgeType::parse("includes").unwrap(),
                EdgeType::parse("supports").unwrap(),
            ]),
        ];
        for direction in [NeighborDirection::Outgoing, NeighborDirection::Incoming] {
            for mode in [
                TraversalMode::Relations,
                TraversalMode::Containment,
                TraversalMode::Both,
            ] {
                assert_stream_query_plans(&db, direction, mode, &filters);
            }
        }
    }

    #[test]
    fn narrow_basis_query_deduplicates_non_null_roots_and_rejects_bad_oids() {
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        db.execute_batch("INSERT INTO nodes(id,kind,uri,hash,created_at,source_snapshot,line_start,line_end,search_hash) VALUES
            ('a.md','vertex','a.md','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused'),
            ('b.md','vertex','b.md','unused',1,NULL,1,1,'unused'),
            ('c.md','vertex','c.md','unused',1,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,1,'unused'),
            ('d.md','vertex','d.md','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused');").unwrap();
        let roots = list_source_snapshots(&db).unwrap();
        assert_eq!(
            roots,
            BTreeSet::from([
                TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
                TreeOid::parse("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()
            ])
        );
        let plan = explain_query(&db, &build_source_snapshots_query());
        println!("basis: {plan:?}");
        assert!(
            plan.iter()
                .any(|line| line.contains("nodes_source_snapshot")),
            "{plan:?}"
        );
        assert!(
            !plan.iter().any(|line| line.contains("TEMP B-TREE")),
            "{plan:?}"
        );
        db.execute(
            "UPDATE nodes SET source_snapshot='malformed' WHERE id='b.md'",
            [],
        )
        .unwrap();
        assert_eq!(
            list_source_snapshots(&db).unwrap_err().get_kind(),
            StorageErrorKind::InternalError
        );
    }

    fn assert_stream_query_plans(
        db: &Connection,
        direction: NeighborDirection,
        mode: TraversalMode,
        filters: &[BTreeSet<EdgeType>],
    ) {
        let cursor = NeighborCursor {
            edge_type: EdgeType::parse("supports").unwrap(),
            neighbor: ElementAddress::parse("last.md").unwrap(),
        };
        for edge_types in filters {
            for after in [None, Some(&cursor)] {
                let request = NeighborPageRequest {
                    node: GraphNodeKey::new(1),
                    direction,
                    mode,
                    edge_types,
                    after,
                    limit: NonZeroUsize::new(32).unwrap(),
                };
                let plan = explain_query(db, &build_neighbor_query(&request));
                println!(
                    "{direction:?}/{mode:?}/{edge_types:?}/cursor={}: {plan:?}",
                    after.is_some()
                );
                let index = match direction {
                    NeighborDirection::Outgoing => "edges_outgoing",
                    NeighborDirection::Incoming => "edges_incoming",
                };
                assert!(
                    plan.iter()
                        .any(|line| line.contains("SEARCH edges USING") && line.contains(index)),
                    "{plan:?}"
                );
                assert!(
                    !plan.iter().any(|line| line.contains("TEMP B-TREE")),
                    "{plan:?}"
                );
            }
        }
    }

    fn explain_query(db: &Connection, query: &SelectStatement) -> Vec<String> {
        let (sql, values) = build_query(query).unwrap();
        assert!(!sql.contains("OFFSET"));
        assert!(!sql.contains("\"hash\""));
        db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(params_from_iter(values), |row| row.get(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
