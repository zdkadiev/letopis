use super::{
    connection::map_sqlite,
    graph::{METAVERTEX_KIND, SOURCE_KIND, VERTEX_KIND, map_invalid_value},
    query::{Edges, Nodes, build_query},
};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, INCLUDES_EDGE_TYPE, KnowledgeId, SourceUri, TreeOid},
    service::{
        ElementAddress, GraphNeighbor, GraphNodeKey, GraphNodeRef, NEIGHBOR_PAGE_QUANTUM,
        NeighborDirection, NeighborPageRequest, StorageError, StorageErrorKind, StorageResult,
        TraversalMode,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Iden)]
enum Endpoint {
    Source,
    Target,
}

pub(super) fn get_node_ref_by_id(
    db: &Connection,
    id: &ElementAddress,
) -> StorageResult<Option<GraphNodeRef>> {
    let (sql, values) = build_query(
        Query::select()
            .columns([Nodes::Id, Nodes::Uri, Nodes::Kind])
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Uri).eq(id.as_str())),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| {
        Ok(decode_node_ref(row))
    })
    .optional()
    .map_err(map_sqlite)?
    .transpose()
}

pub(super) fn get_neighbor_page(
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

pub(super) fn get_source_snapshots(db: &Connection) -> StorageResult<BTreeSet<TreeOid>> {
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
    use super::{
        build_neighbor_query, build_source_snapshots_query, get_neighbor_page, get_source_snapshots,
    };
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

    #[derive(Debug)]
    struct Instruction {
        address: i64,
        opcode: String,
        p1: i64,
        p2: i64,
        p4: Option<String>,
    }

    #[test]
    fn pages_retain_limit_bounded_top_k_for_every_query_variant() {
        // Given: 4096 neighbors in reverse numeric/URI order and unresolved declarations.
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        create_high_degree_graph(&db);
        let filters = [
            BTreeSet::new(),
            BTreeSet::from([EdgeType::parse("supports").unwrap()]),
            BTreeSet::from([
                EdgeType::parse("includes").unwrap(),
                EdgeType::parse("supports").unwrap(),
            ]),
        ];
        let cases = [
            (
                TraversalMode::Relations,
                [Some("refutes"), Some("supports"), Some("supports")],
            ),
            (
                TraversalMode::Containment,
                [Some("includes"), None, Some("includes")],
            ),
            (
                TraversalMode::Both,
                [Some("includes"), Some("supports"), Some("includes")],
            ),
        ];
        // When / Then: all 72 directional/mode/filter/cursor/limit variants use bounded native bytecode and URI pages.
        println!(
            "bundled SQLite {}: high_degree=4096, variants=72",
            rusqlite::version()
        );
        for direction in [NeighborDirection::Outgoing, NeighborDirection::Incoming] {
            for (mode, first_types) in cases {
                for (edge_types, first_type) in filters.iter().zip(first_types) {
                    assert_bounded_query_variants(&db, direction, mode, edge_types, first_type);
                }
            }
        }
    }

    #[test]
    fn narrow_basis_query_deduplicates_non_null_roots_and_rejects_bad_oids() {
        // Given: knowledge rows share some snapshot roots and omit another.
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        db.execute_batch("insert into nodes(uri,kind,hash,created_at,source_snapshot,line_start,line_end,search_hash) values
            ('a.md','vertex','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused'),
            ('b.md','vertex','unused',1,null,1,1,'unused'),
            ('c.md','vertex','unused',1,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,1,'unused'),
            ('d.md','vertex','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused');").unwrap();
        // When: the narrow snapshot projection reads distinct roots.
        let roots = get_source_snapshots(&db).unwrap();
        // Then: the sorted roots are unique and use the snapshot index.
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
        // Given: introduce a malformed snapshot identity.
        db.execute(
            "update nodes set source_snapshot='malformed' where uri='b.md'",
            [],
        )
        .unwrap();
        // When / Then: a malformed persisted root is rejected.
        assert_eq!(
            get_source_snapshots(&db).unwrap_err().get_kind(),
            StorageErrorKind::InternalError
        );
    }

    fn create_high_degree_graph(db: &Connection) {
        db.execute_batch("insert into nodes(id,uri,kind,hash,created_at,line_start,line_end,search_hash)
            values(10000,'root.md','vertex','unused',1,1,1,'unused');
            with recursive ordinal(n) as (values(0) union all select n+1 from ordinal where n<4095)
            insert into nodes(id,uri,kind,hash,created_at,line_start,line_end,search_hash)
            select 4096-n,printf('n%04d.md',n),'vertex','unused',1,1,1,'unused' from ordinal;
            insert into edges select 10000,id,'supports' from nodes where id<>10000;
            insert into edges select 10000,id,'refutes' from nodes where id<>10000;
            insert into edges select 10000,id,'includes' from nodes where id<>10000;
            insert into edges select id,10000,'supports' from nodes where id<>10000;
            insert into edges select id,10000,'refutes' from nodes where id<>10000;
            insert into edges select id,10000,'includes' from nodes where id<>10000;
            insert into unresolved_edges values(10000,'a-missing.md','includes'),(10000,'a-missing.md','supports');").unwrap();
    }

    fn assert_bounded_query_variants(
        db: &Connection,
        direction: NeighborDirection,
        mode: TraversalMode,
        edge_types: &BTreeSet<EdgeType>,
        first_type: Option<&str>,
    ) {
        let cursor = NeighborCursor {
            edge_type: EdgeType::parse("supports").unwrap(),
            neighbor: ElementAddress::parse("n2047.md").unwrap(),
        };
        for after in [None, Some(&cursor)] {
            for limit in [1, 32] {
                let request = NeighborPageRequest {
                    node: GraphNodeKey::new(10000),
                    direction,
                    mode,
                    edge_types,
                    after,
                    limit: NonZeroUsize::new(limit).unwrap(),
                };
                let query = build_neighbor_query(&request);
                let plan = explain_query(db, &query);
                assert_directional_index(&plan, direction);
                assert_bounded_vm(db, &query, limit, edge_types.len());
                let expected_type = match (after, mode) {
                    (Some(_), TraversalMode::Containment) => None,
                    (Some(_), _) => Some("supports"),
                    (None, _) => first_type,
                };
                let first = if after.is_some() { 2048 } else { 0 };
                let expected: Vec<_> = expected_type
                    .into_iter()
                    .flat_map(|kind| {
                        (first..first + limit)
                            .map(move |n| (kind.to_owned(), format!("n{n:04}.md")))
                    })
                    .collect();
                let actual: Vec<_> = get_neighbor_page(db, &request)
                    .unwrap()
                    .into_iter()
                    .map(|row| {
                        (
                            row.edge.get_edge_type().as_str().to_owned(),
                            row.node.element.as_str().to_owned(),
                        )
                    })
                    .collect();
                assert_eq!(actual, expected);
                println!(
                    "{direction:?}/{mode:?}/{edge_types:?}/cursor={}/limit={limit}: {plan:?}; top-K <= {limit}",
                    after.is_some()
                );
            }
        }
    }

    fn assert_directional_index(plan: &[String], direction: NeighborDirection) {
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
            !plan
                .iter()
                .any(|line| line.contains("AUTOMATIC") || line.contains("MATERIALIZE")),
            "{plan:?}"
        );
    }

    fn assert_bounded_vm(
        db: &Connection,
        query: &SelectStatement,
        limit: usize,
        filter_count: usize,
    ) {
        let program = get_query_program(db, query);
        assert!(
            !program.iter().any(|op| op.opcode.starts_with("Sorter")
                || matches!(
                    op.opcode.as_str(),
                    "OpenAutoindex"
                        | "OffsetLimit"
                        | "RowSetAdd"
                        | "Insert"
                        | "OpenDup"
                        | "OpenPseudo"
                )),
            "{program:#?}"
        );
        for open in program.iter().filter(|op| op.opcode == "OpenEphemeral") {
            assert!(open.p4.as_deref().is_some_and(|key| key.starts_with("k(")));
            let inserts: Vec<_> = program
                .iter()
                .filter(|op| op.opcode == "IdxInsert" && op.p1 == open.p1)
                .collect();
            if program
                .iter()
                .any(|op| op.opcode == "Sequence" && op.p1 == open.p1)
            {
                assert_eq!(inserts.len(), 1, "{program:#?}");
                assert_top_k_control_flow(&program, inserts[0], limit);
            } else {
                assert_constant_filter(&program, open, &inserts, filter_count);
            }
        }
        assert!(
            program.iter().any(|op| op.opcode == "IfNotZero"),
            "{program:#?}"
        );
        if limit == 32 && filter_count == 0 {
            println!(
                "native VM: {:?}",
                program
                    .iter()
                    .map(|op| (
                        op.address,
                        op.opcode.as_str(),
                        op.p1,
                        op.p2,
                        op.p4.as_deref()
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }

    fn get_query_program(db: &Connection, query: &SelectStatement) -> Vec<Instruction> {
        let (sql, values) = build_query(query).unwrap();
        db.prepare(&format!("explain {sql}"))
            .unwrap()
            .query_map(params_from_iter(&values), |row| {
                Ok(Instruction {
                    address: row.get(0)?,
                    opcode: row.get(1)?,
                    p1: row.get(2)?,
                    p2: row.get(3)?,
                    p4: row.get(5)?,
                })
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn assert_top_k_control_flow(program: &[Instruction], insert: &Instruction, limit: usize) {
        let gate = program
            .iter()
            .rfind(|op| op.opcode == "IfNotZero" && op.address < insert.address)
            .unwrap_or_else(|| panic!("{program:#?}"));
        let start = usize::try_from(gate.address).unwrap();
        let sequence = &program[start..=start + 3];
        // Pinned SQLite pushOntoSorter: consume LIMIT slots, then discard or delete the largest before inserting.
        assert_eq!(
            sequence
                .iter()
                .map(|op| op.opcode.as_str())
                .collect::<Vec<_>>(),
            ["IfNotZero", "Last", "IdxLE", "Delete"]
        );
        assert_eq!(gate.p2, gate.address + 4);
        for op in &sequence[1..=3] {
            assert_eq!(op.p1, insert.p1);
        }
        assert!(sequence[2].p2 > insert.address);
        assert!(
            program[start + 4..usize::try_from(insert.address).unwrap()]
                .iter()
                .all(|op| matches!(
                    op.opcode.as_str(),
                    "Column" | "Rowid" | "IdxRowid" | "MakeRecord"
                )),
            "{program:#?}"
        );
        let limit_initialization = program
            .iter()
            .find(|op| op.opcode == "Integer" && op.p2 == gate.p1)
            .unwrap_or_else(|| panic!("{program:#?}"));
        assert_eq!(limit_initialization.p1, i64::try_from(limit).unwrap());
    }

    fn assert_constant_filter(
        program: &[Instruction],
        open: &Instruction,
        inserts: &[&Instruction],
        filter_count: usize,
    ) {
        assert_eq!(inserts.len(), filter_count);
        assert!(filter_count <= 2);
        assert_eq!(open.p2, 1);
        let once = &program[usize::try_from(open.address - 1).unwrap()];
        assert_eq!(once.opcode, "Once");
        assert!(inserts.iter().all(|op| op.address < once.p2));
        let end = usize::try_from(inserts.last().unwrap().address).unwrap();
        assert!(
            program[usize::try_from(open.address).unwrap()..=end]
                .iter()
                .all(|op| matches!(
                    op.opcode.as_str(),
                    "OpenEphemeral" | "Variable" | "MakeRecord" | "IdxInsert"
                ))
        );
        for insert in inserts {
            let index = usize::try_from(insert.address).unwrap();
            assert_eq!(program[index - 2].opcode, "Variable");
            assert_eq!(program[index - 1].opcode, "MakeRecord");
        }
    }

    fn explain_query(db: &Connection, query: &SelectStatement) -> Vec<String> {
        let (sql, values) = build_query(query).unwrap();
        assert!(!sql.contains("OFFSET"));
        assert!(!sql.contains("\"hash\""));
        db.prepare(&format!("explain query plan {sql}"))
            .unwrap()
            .query_map(params_from_iter(values), |row| row.get(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
