#[path = "support/storage.rs"]
mod storage_support;

use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId},
    open_storage,
    service::{NodeMetadata, Storage, StorageErrorKind},
};
use rusqlite::{Connection, ErrorCode};
use storage_support::build_knowledge;
use tempfile::TempDir;

#[test]
fn node_schema_uses_integer_id_and_unique_uri() {
    // Given: a freshly initialized cache and an independent SQLite connection.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let _storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let db = Connection::open(&cache).unwrap();
    // When: SQLite describes the persisted identity columns and endpoint foreign keys.
    let columns: Vec<(String, String, i64)> = db
        .prepare("pragma table_info(nodes)")
        .unwrap()
        .query_map([], |row| Ok((row.get(1)?, row.get(2)?, row.get(5)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let endpoints: Vec<(String, String, String)> = db
        .prepare("pragma foreign_key_list(edges)")
        .unwrap()
        .query_map([], |row| Ok((row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // Then: one integer primary key and one textual address own node identity.
    assert!(columns.contains(&("id".into(), "INTEGER".into(), 1)));
    assert!(!columns.iter().any(|(name, _, _)| name == "row_id"));
    assert_eq!(
        endpoints,
        vec![
            ("nodes".into(), "target_id".into(), "id".into()),
            ("nodes".into(), "source_id".into(), "id".into()),
        ]
    );
    assert_eq!(
        db.query_row("pragma user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
    // When: two explicit distinct numeric keys try to claim the same address.
    db.execute(
        "insert into nodes(id,uri,kind,created_at) values(41,'code.rs','source',0)",
        [],
    )
    .unwrap();
    let duplicate = db
        .execute(
            "insert into nodes(id,uri,kind,created_at) values(42,'code.rs','source',0)",
            [],
        )
        .unwrap_err();
    // Then: the URI constraint rejects the second row independently of UPSERT.
    assert_eq!(
        duplicate.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    );
    assert_eq!(
        db.query_row(
            "select count(*) from nodes where uri='code.rs'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn resolved_edges_reference_integer_node_keys() {
    // Given: explicit numeric keys differ from lexical URI order, with FK checks enabled.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let _storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let db = Connection::open(&cache).unwrap();
    db.execute_batch(
        "pragma foreign_keys=on;
        insert into nodes(id,uri,kind,created_at) values
        (41,'z.rs','source',0),(7,'a.rs','source',0);",
    )
    .unwrap();
    // When: inspect endpoint types and insert independent literal integer endpoints.
    let columns = get_column_types(&db, "edges");
    let foreign_keys = get_foreign_keys(&db, "edges");
    // Then: both endpoints are restrictive integer references to nodes.id.
    assert_eq!(
        columns,
        vec![
            ("source_id".into(), "INTEGER".into()),
            ("target_id".into(), "INTEGER".into()),
            ("type".into(), "TEXT".into())
        ]
    );
    assert_eq!(
        foreign_keys,
        vec![
            (
                "nodes".into(),
                "target_id".into(),
                "id".into(),
                "NO ACTION".into()
            ),
            (
                "nodes".into(),
                "source_id".into(),
                "id".into(),
                "NO ACTION".into()
            )
        ]
    );
    // When: independent literal integer endpoints are inserted.
    db.execute("insert into edges values(41,7,'supports')", [])
        .unwrap();
    // Then: the raw triple uses those keys and invalid endpoint mutations are rejected.
    assert_eq!(get_resolved_rows(&db), vec![(41, 7, "supports".into())]);
    for sql in [
        "insert into edges values(999,7,'supports')",
        "insert into edges values(41,999,'supports')",
        "insert into edges values(41,41,'supports')",
    ] {
        assert_eq!(
            db.execute(sql, []).unwrap_err().sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation)
        );
    }
    assert_eq!(
        db.query_row("select count(*) from pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn edge_before_target_resolves_at_commit_without_a_placeholder() {
    // Given: a declaration is inserted before its target within one write unit.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("z.md", "owner"))
        .unwrap();
    write.insert_edge(&build_edge("z.md", "a.md")).unwrap();
    // When: the target arrives later and the unit publishes.
    write
        .upsert_node(&build_knowledge("a.md", "target"))
        .unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    // Then: one numeric edge represents the unchanged textual declaration.
    let db = Connection::open(&cache).unwrap();
    assert_eq!(get_resolved_rows(&db), vec![(1, 2, "supports".into())]);
    assert!(get_unresolved_rows(&db).is_empty());
    assert_eq!(
        get_public_triples(&mut storage),
        vec![("z.md".into(), "a.md".into(), "supports".into())]
    );
}

#[test]
fn absent_target_retains_a_declaration_without_a_fake_node() {
    // Given: only the owner exists and the target never arrives.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "owner"))
        .unwrap();
    write
        .insert_edge(&build_edge("a.md", "missing.md"))
        .unwrap();
    // When: commit publishes the still-unresolved declaration.
    write.commit().unwrap();
    // Then: it remains reportable, while only the owner's metadata exists.
    let db = Connection::open(&cache).unwrap();
    assert!(get_resolved_rows(&db).is_empty());
    assert_eq!(
        get_unresolved_rows(&db),
        vec![(1, "missing.md".into(), "supports".into())]
    );
    assert_eq!(
        db.query_row("select count(*) from nodes", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        get_public_triples(&mut storage),
        vec![("a.md".into(), "missing.md".into(), "supports".into())]
    );
    assert_eq!(
        db.query_row("select count(*) from pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        get_foreign_keys(&db, "unresolved_edges"),
        vec![(
            "nodes".into(),
            "source_id".into(),
            "id".into(),
            "NO ACTION".into()
        )]
    );
    db.execute_batch("pragma foreign_keys=on").unwrap();
    assert_eq!(
        db.execute(
            "insert into unresolved_edges values(999,'missing.md','supports')",
            []
        )
        .unwrap_err()
        .sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    );
}

#[test]
fn duplicate_after_target_upsert_poisoning_preserves_pending_declaration() {
    // Given: revision one retains an unresolved declaration.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "owner"))
        .unwrap();
    write.insert_edge(&build_edge("a.md", "b.md")).unwrap();
    write.commit().unwrap();
    // When: the target appears before a duplicate insertion is attempted.
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("b.md", "target"))
        .unwrap();
    let rejected = write.insert_edge(&build_edge("a.md", "b.md"));
    let publication = write.commit();
    // Then: the duplicate is typed InvalidRequest and all staged changes roll back.
    assert_eq!(
        rejected.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert_eq!(
        publication.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    let db = Connection::open(&cache).unwrap();
    assert!(get_resolved_rows(&db).is_empty());
    assert_eq!(
        get_unresolved_rows(&db),
        vec![(1, "b.md".into(), "supports".into())]
    );
    assert_eq!(
        db.query_row("select revision from project", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        get_public_triples(&mut storage),
        vec![("a.md".into(), "b.md".into(), "supports".into())]
    );
}

#[test]
fn target_deletion_demotes_and_reappearance_uses_a_new_integer_key() {
    // Given: a resolved declaration refers to numeric target key two.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    for uri in ["a.md", "b.md"] {
        write.upsert_node(&build_knowledge(uri, "text")).unwrap();
    }
    write.insert_edge(&build_edge("a.md", "b.md")).unwrap();
    write.commit().unwrap();
    // When: delete the target, then publish its reappearance in a later unit.
    let mut write = storage.begin_cache_write_tx().unwrap();
    write.delete_node(&build_target("b.md")).unwrap();
    write.commit().unwrap();
    let db = Connection::open(&cache).unwrap();
    // Then: the declaration is demoted without losing its logical identity.
    assert!(get_resolved_rows(&db).is_empty());
    assert_eq!(
        get_unresolved_rows(&db),
        vec![(1, "b.md".into(), "supports".into())]
    );
    assert_eq!(
        get_public_triples(&mut storage),
        vec![("a.md".into(), "b.md".into(), "supports".into())]
    );
    // When: the target returns with a fresh key.
    let mut write = storage.begin_cache_write_tx().unwrap();
    assert_eq!(
        write.upsert_node(&build_knowledge("b.md", "new")).unwrap(),
        3
    );
    write.commit().unwrap();
    // Then: only the fresh integer key is referenced and the pending row is gone.
    assert_eq!(get_resolved_rows(&db), vec![(1, 3, "supports".into())]);
    assert!(get_unresolved_rows(&db).is_empty());
    assert_eq!(
        get_public_triples(&mut storage),
        vec![("a.md".into(), "b.md".into(), "supports".into())]
    );
    assert_eq!(
        db.query_row("select count(*) from pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn pending_outgoing_declarations_block_deletion_until_owner_cleanup() {
    // Given: an owner has both a resolved and an unresolved outgoing declaration.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    for uri in ["a.md", "b.md"] {
        write.upsert_node(&build_knowledge(uri, "alpha")).unwrap();
    }
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    write.insert_edge(&build_edge("a.md", "b.md")).unwrap();
    write
        .insert_edge(&build_edge("a.md", "missing.md"))
        .unwrap();
    write.commit().unwrap();
    // When: deletion is attempted before cleaning the owner.
    let mut write = storage.begin_cache_write_tx().unwrap();
    assert_eq!(
        write
            .delete_node(&build_target("a.md"))
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    // Then: both representations and searchable text survive the poisoned unit.
    let db = Connection::open(&cache).unwrap();
    assert_eq!(get_resolved_rows(&db), vec![(1, 2, "supports".into())]);
    assert_eq!(
        get_unresolved_rows(&db),
        vec![(1, "missing.md".into(), "supports".into())]
    );
    assert_eq!(
        storage
            .begin_read_tx()
            .unwrap()
            .get_text_matches_by_expression("alpha")
            .unwrap()
            .len(),
        1
    );
    // When: owner cleanup removes both representations and missing-node deletion is a no-op.
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("a.md").unwrap())
        .unwrap();
    write.delete_node(&build_target("a.md")).unwrap();
    write.delete_node(&build_target("never.md")).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    // Then: no declaration, owner or stale FTS row remains.
    assert!(get_resolved_rows(&db).is_empty());
    assert!(get_unresolved_rows(&db).is_empty());
    assert!(get_public_triples(&mut storage).is_empty());
    assert!(
        storage
            .begin_read_tx()
            .unwrap()
            .get_text_matches_by_expression("alpha")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unequal_knowledge_identity_poisoning_rolls_back_publication() {
    // Given: revision one has one searchable node and a captured initial FTS result.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut initial = storage.begin_cache_write_tx().unwrap();
    initial
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    initial
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    assert_eq!(initial.commit().unwrap(), 1);
    let mut read = storage.begin_read_tx().unwrap();
    let before = read.get_text_matches_by_expression("alpha").unwrap();
    assert_eq!(before.len(), 1);
    let initial_row = before[0].row_id;
    drop(read);
    let unequal = build_unequal_knowledge();
    // When: a writer stages valid work, catches an invalid identity, then tries to publish.
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("pending.md", "pending"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("pending.md").unwrap(), "pending")
        .unwrap();
    let rejected = write.upsert_node(&unequal);
    let published = write.commit();
    // Then: rejection poisons the unit, preserving revision one and its search projection.
    assert_eq!(
        rejected.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert_eq!(
        published.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.get_nodes().unwrap().len(), 1);
    for id in ["pending.md", "a.md#part"] {
        assert!(
            read.get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
                .unwrap()
                .is_none()
        );
    }
    let after = read.get_text_matches_by_expression("alpha").unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id.as_str(), "a.md");
    assert_eq!(after[0].row_id, initial_row);
    assert_eq!(after[0].bm25, before[0].bm25);
    assert!(
        read.get_text_matches_by_expression("pending OR bad")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn missing_edge_source_poisoning_rolls_back_staged_nodes() {
    // Given: a write unit has a target but no owner for the declaration.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("b.md", "target"))
        .unwrap();
    // When: insertion uses an absent source and the caller attempts to commit.
    let rejected = write.insert_edge(&build_edge("missing.md", "b.md"));
    let published = write.commit();
    // Then: both failures are typed invalid requests and the staged target rolls back.
    assert_eq!(
        rejected.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert_eq!(
        published.unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 0);
    assert!(read.get_nodes().unwrap().is_empty());
    assert!(read.get_edges_by_source_id(None).unwrap().is_empty());
}

fn build_unequal_knowledge() -> NodeMetadata {
    let mut unequal = build_knowledge("a.md#part", "bad");
    let NodeMetadata::Knowledge(metadata) = &mut unequal else {
        panic!()
    };
    metadata.uri = KnowledgeId::parse("a.md").unwrap();
    unequal
}

fn build_edge(source: &str, target: &str) -> Edge {
    Edge::new(
        KnowledgeId::parse(source).unwrap(),
        build_target(target),
        EdgeType::parse("supports").unwrap(),
    )
    .unwrap()
}

fn build_target(uri: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(uri).unwrap())
}

fn get_resolved_rows(db: &Connection) -> Vec<(i64, i64, String)> {
    db.prepare("select source_id,target_id,type from edges order by source_id,target_id,type")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn get_unresolved_rows(db: &Connection) -> Vec<(i64, String, String)> {
    db.prepare(
        "select source_id,target_uri,type from unresolved_edges order by source_id,target_uri,type",
    )
    .unwrap()
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn get_public_triples(storage: &mut dyn Storage) -> Vec<(String, String, String)> {
    storage
        .begin_read_tx()
        .unwrap()
        .get_edges_by_source_id(None)
        .unwrap()
        .into_iter()
        .map(|edge| {
            (
                edge.get_source().as_str().into(),
                edge.get_target().as_str().into(),
                edge.get_edge_type().as_str().into(),
            )
        })
        .collect()
}

fn get_column_types(db: &Connection, table: &str) -> Vec<(String, String)> {
    db.prepare(&format!("pragma table_info({table})"))
        .unwrap()
        .query_map([], |row| Ok((row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn get_foreign_keys(db: &Connection, table: &str) -> Vec<(String, String, String, String)> {
    db.prepare(&format!("pragma foreign_key_list({table})"))
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(2)?, row.get(3)?, row.get(4)?, row.get(6)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
