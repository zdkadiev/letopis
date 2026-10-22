#[path = "support/storage.rs"]
mod storage_support;
use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    open_storage,
    service::{CacheWrite, Storage, StorageRead},
};
use storage_support::{build_file, build_knowledge, build_source};
use tempfile::TempDir;
#[test]
fn cache_commit_preserves_node_row_id() {
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut write = storage.begin_cache_write().unwrap();
    let first = write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    let mut write = storage.begin_cache_write().unwrap();
    let second = write.upsert_node(&build_knowledge("a.md", "beta")).unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "beta")
        .unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    let mut read = storage.begin_read().unwrap();
    assert_eq!(second, first);
    assert_eq!(read.match_text("alpha").unwrap().len(), 0);
    assert_eq!(read.match_text("beta").unwrap().len(), 1);
    assert_eq!(
        read.read_node(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
            .unwrap()
            .unwrap()
            .row_id,
        first
    );
}

use letopis::domain::{Edge, EdgeType, RepoPath, Sha256};
use letopis::service::{NodeMetadata, StorageErrorKind};
use rusqlite::Connection;
use std::error::Error;
fn build_target(id: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
}
fn build_edge(source: &str, target: EdgeTarget, kind: &str) -> Edge {
    Edge::new(
        KnowledgeId::parse(source).unwrap(),
        target,
        EdgeType::parse(kind).unwrap(),
    )
    .unwrap()
}

#[test]
fn source_missing_bytes_and_declared_targets_round_trip() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let source = build_source("code.rs#L2-L4", None, 3);
    let NodeMetadata::Source(ref meta) = source else {
        panic!()
    };
    let id = EdgeTarget::Source(meta.uri.clone());
    let external = build_edge("a.md", id.clone(), "external");
    let dangling = build_edge("a.md", build_target("missing.md"), "supports");
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_node(&source).unwrap();
    write.insert_edge(&external).unwrap();
    write.insert_edge(&dangling).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.read_node(&id).unwrap().unwrap().metadata, source);
    assert_eq!(read.list_edges(None).unwrap(), vec![external, dangling]);
    assert!(
        read.read_node(&build_target("missing.md"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn incoming_edges_survive_target_delete_and_reappear() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let declared = build_edge("a.md", build_target("b.md"), "supports");
    let mut write = db.begin_cache_write().unwrap();
    for id in ["a.md", "b.md"] {
        write.upsert_node(&build_knowledge(id, "alpha")).unwrap();
    }
    write.insert_edge(&declared).unwrap();
    write.commit().unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("b.md").unwrap())
        .unwrap();
    write.delete_node(&build_target("b.md")).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.list_edges(None).unwrap(), vec![declared.clone()]);
    assert!(read.read_node(&build_target("b.md")).unwrap().is_none());
    drop(read);
    let mut write = db.begin_cache_write().unwrap();
    write.upsert_node(&build_knowledge("b.md", "beta")).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    assert!(read.read_node(&build_target("b.md")).unwrap().is_some());
    assert_eq!(
        read.list_edges(Some(&KnowledgeId::parse("a.md").unwrap()))
            .unwrap(),
        vec![declared]
    );
}
#[test]
fn source_projection_can_be_recomputed_and_removed() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let id = EdgeTarget::Source(letopis::domain::SourceUri::parse("code.rs").unwrap());
    let mut first = build_knowledge("a.md", "alpha");
    if let NodeMetadata::Knowledge(ref mut meta) = first {
        meta.created_at = 3;
    }
    let updated = build_source("code.rs", Some(Sha256::digest(b"bytes")), 7);
    // Arrange: two referring nodes; caller supplies their minimum creation time.
    let mut write = db.begin_cache_write().unwrap();
    write.upsert_node(&first).unwrap();
    write.upsert_node(&build_knowledge("b.md", "beta")).unwrap();
    write
        .upsert_node(&build_source("code.rs", None, 3))
        .unwrap();
    for source in ["a.md", "b.md"] {
        write
            .insert_edge(&build_edge(source, id.clone(), "external"))
            .unwrap();
    }
    write.commit().unwrap();
    // Act: removing the oldest referrer recomputes the projection in the same unit.
    let mut write = db.begin_cache_write().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("a.md").unwrap())
        .unwrap();
    write.delete_node(&build_target("a.md")).unwrap();
    write.upsert_node(&updated).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.read_node(&id).unwrap().unwrap().metadata, updated);
    assert_eq!(
        read.list_edges(None).unwrap(),
        vec![build_edge("b.md", id.clone(), "external")]
    );
    drop(read);
    // Act and assert: caller removes the last reference and source together.
    let mut write = db.begin_cache_write().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("b.md").unwrap())
        .unwrap();
    write.delete_node(&id).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    assert!(read.read_node(&id).unwrap().is_none());
    assert!(read.list_edges(None).unwrap().is_empty());
}
#[test]
fn valid_invalid_and_unmarked_file_rows_round_trip() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let valid = build_file("a.md", "alpha");
    let mut invalid = build_file("b.md", "beta");
    invalid.valid = false;
    invalid.error = Some("invalid metadata".into());
    let unmarked = build_file("unmarked.md", "plain");
    let mut write = db.begin_cache_write().unwrap();
    for row in [&valid, &invalid, &unmarked] {
        write.upsert_file(row).unwrap();
    }
    write.commit().unwrap();
    let mut read = db.begin_read().unwrap();
    for row in [&valid, &invalid, &unmarked] {
        assert_eq!(read.read_file(&row.path).unwrap().as_ref(), Some(row));
    }
    drop(read);
    let mut write = db.begin_cache_write().unwrap();
    write.delete_file(&valid.path).unwrap();
    write.commit().unwrap();
    assert!(
        db.begin_read()
            .unwrap()
            .read_file(&valid.path)
            .unwrap()
            .is_none()
    );
}
#[test]
fn caught_mutation_error_poisoned_unit_cannot_commit() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    assert_eq!(
        write
            .insert_edge(&build_edge("missing.md", build_target("a.md"), "supports"))
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    assert_empty(&mut db);
}
fn assert_empty(db: &mut impl Storage) {
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.get_revision(), 0);
    assert!(read.read_node(&build_target("a.md")).unwrap().is_none());
    assert!(
        read.read_file(&RepoPath::parse("a.md").unwrap())
            .unwrap()
            .is_none()
    );
    assert!(read.match_text("alpha").unwrap().is_empty());
}
#[test]
fn dropped_unit_rolls_back() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    {
        let mut write = db.begin_cache_write().unwrap();
        write
            .upsert_node(&build_knowledge("a.md", "alpha"))
            .unwrap();
        write
            .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
            .unwrap();
        write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    }
    assert_empty(&mut db);
}
#[test]
fn late_fts_error_rolls_back_all_changes() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    assert!(
        write
            .replace_text(&KnowledgeId::parse("missing.md").unwrap(), "alpha")
            .is_err()
    );
    assert!(write.commit().is_err());
    assert_empty(&mut db);
}
#[test]
fn unsigned_values_do_not_wrap() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut node = build_knowledge("a.md", "alpha");
    let NodeMetadata::Knowledge(ref mut meta) = node else {
        panic!()
    };
    meta.created_at = u64::MAX;
    let mut write = db.begin_cache_write().unwrap();
    assert!(write.upsert_node(&node).is_err());
    assert!(write.commit().is_err());
    for field in ["size", "inode"] {
        let mut file = build_file("a.md", "alpha");
        if field == "size" {
            file.size = u64::MAX;
        } else {
            file.inode = u64::MAX;
        }
        let mut write = db.begin_cache_write().unwrap();
        assert!(write.upsert_file(&file).is_err());
        assert!(write.commit().is_err());
    }
    assert_empty(&mut db);
}
#[test]
fn revision_overflow_rolls_back() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    Connection::open(&cache)
        .unwrap()
        .execute("UPDATE project SET revision=?1", [i64::MAX])
        .unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    assert!(write.commit().is_err());
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.get_revision(), i64::MAX as u64);
    assert!(read.read_node(&build_target("a.md")).unwrap().is_none());
}
#[test]
fn duplicate_edges_and_invalid_checks_are_rejected() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    let e = build_edge("a.md", build_target("b.md"), "supports");
    write.insert_edge(&e).unwrap();
    assert_eq!(
        write.insert_edge(&e).unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    let sql = Connection::open(&cache).unwrap();
    for query in [
        "INSERT INTO nodes(id,kind,uri,created_at) VALUES('bad','source','different',1)",
        "INSERT INTO nodes(id,kind,uri,created_at,source_snapshot) VALUES('bad','source','bad',1,'snapshot')",
        "INSERT INTO nodes(id,kind,uri,created_at,search_hash) VALUES('bad','source','bad',1,'hash')",
        "INSERT INTO nodes(id,kind,uri,created_at,line_start) VALUES('bad','source','bad',1,1)",
        "INSERT INTO nodes(id,kind,uri,created_at) VALUES('bad','vertex','bad',1)",
        "INSERT INTO nodes(id,kind,uri,created_at,line_start,line_end) VALUES('bad','source','bad',1,0,1)",
        "INSERT INTO files VALUES('x',1,2,3,'hash',4,1,'error')",
        "INSERT INTO files VALUES('x',1,2,3,'hash',4,0,NULL)",
        "INSERT INTO files VALUES('x',1,2,3,'hash',4,2,NULL)",
    ] {
        assert!(sql.execute(query, []).is_err());
    }
}
#[test]
fn contentless_fts_does_not_return_original_text() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "privatepayload"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "privatepayload")
        .unwrap();
    write.commit().unwrap();
    let sql = Connection::open(&cache).unwrap();
    assert_eq!(
        sql.query_row("SELECT text FROM text_index", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    let columns: Vec<String>=sql.prepare("SELECT name FROM pragma_table_info('nodes') UNION ALL SELECT name FROM pragma_table_info('files')").unwrap().query_map([],|r|r.get(0)).unwrap().map(Result::unwrap).collect();
    assert!(
        !columns
            .iter()
            .any(|c| ["text", "body", "content"].contains(&c.as_str()))
    );
    let mut read = db.begin_read().unwrap();
    assert_eq!(read.match_text("privatepayload").unwrap().len(), 1);
    let expression = "privatepayload Authorization:Bearer-secret \"";
    let error = read.match_text(expression).unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(error.source().is_some());
    for marker in ["privatepayload", "Authorization", expression] {
        assert!(!error.to_string().contains(marker));
        assert!(!error.get_message().contains(marker));
    }
}

#[test]
fn empty_cache_publications_preserve_vector_identity_and_state() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let vectors = root.path().join("v");
    let mut db = open_storage(&cache, &vectors).unwrap();
    let sql = Connection::open(&vectors).unwrap();
    sql.execute("INSERT INTO meta VALUES(1,?1)", ["a".repeat(64)])
        .unwrap();
    sql.execute(
        "INSERT INTO embed_failures VALUES(?1,2,'safe')",
        ["b".repeat(64)],
    )
    .unwrap();
    // Act: even an empty explicitly committed unit advances only the cache revision.
    assert_eq!(db.begin_cache_write().unwrap().commit().unwrap(), 1);
    assert_eq!(db.begin_cache_write().unwrap().commit().unwrap(), 2);
    let read = db.begin_read().unwrap();
    assert_eq!(read.get_revision(), 2);
    assert_eq!(
        read.get_config_identity(),
        Some(Sha256::parse(&"a".repeat(64)).unwrap())
    );
    assert_eq!(
        sql.query_row("SELECT attempts FROM embed_failures", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
#[test]
fn fts_matches_use_native_score_then_id_without_candidate_limit() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    for id in ["z.md", "b.md", "a.md"] {
        write.upsert_node(&build_knowledge(id, "alpha")).unwrap();
        write
            .replace_text(&KnowledgeId::parse(id).unwrap(), "alpha")
            .unwrap();
    }
    write
        .upsert_node(&build_knowledge("strong.md", "alpha alpha alpha"))
        .unwrap();
    write
        .replace_text(
            &KnowledgeId::parse("strong.md").unwrap(),
            "alpha alpha alpha",
        )
        .unwrap();
    write.commit().unwrap();
    let results = db.begin_read().unwrap().match_text("alpha").unwrap();
    assert_eq!(
        results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["strong.md", "a.md", "b.md", "z.md"]
    );
    assert!(results[0].bm25 < results[1].bm25);
}
#[test]
fn malformed_persisted_values_return_internal_errors() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    write.commit().unwrap();
    let sql = Connection::open(&cache).unwrap();
    for update in [
        "UPDATE nodes SET hash='invalid'",
        "UPDATE nodes SET hash='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',created_at=-1",
    ] {
        sql.execute(update, []).unwrap();
        assert_eq!(
            db.begin_read()
                .unwrap()
                .read_node(&build_target("a.md"))
                .unwrap_err()
                .get_kind(),
            StorageErrorKind::InternalError
        );
    }
    sql.execute("UPDATE files SET inode=-1", []).unwrap();
    assert_eq!(
        db.begin_read()
            .unwrap()
            .read_file(&RepoPath::parse("a.md").unwrap())
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InternalError
    );
    sql.execute("INSERT INTO edges VALUES('a.md','b.md','INVALID')", [])
        .unwrap();
    assert_eq!(
        db.begin_read()
            .unwrap()
            .list_edges(None)
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InternalError
    );
}

#[test]
fn knowledge_and_source_optional_fields_survive_reopen() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let vectors = root.path().join("v");
    let mut knowledge = build_knowledge("a.md#Section", "alpha");
    if let NodeMetadata::Knowledge(ref mut n) = knowledge {
        n.uri = KnowledgeId::parse("a.md").unwrap();
        n.kind = letopis::domain::ElementKind::Metavertex;
        n.created_at = 0;
        n.source_snapshot = Some(letopis::domain::TreeOid::parse(&"a".repeat(40)).unwrap());
        n.lines = letopis::domain::LineRange::new(2, 8).unwrap();
    }
    let mut source = build_source("code.rs#L2-L4", Some(Sha256::digest(b"bytes")), 3);
    if let NodeMetadata::Source(ref mut n) = source {
        n.lines = Some(letopis::domain::LineRange::new(2, 4).unwrap());
    }
    let source_id = EdgeTarget::Source(letopis::domain::SourceUri::parse("code.rs#L2-L4").unwrap());
    let mut db = open_storage(&cache, &vectors).unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write.upsert_node(&knowledge).unwrap();
    write.upsert_node(&source).unwrap();
    write.commit().unwrap();
    drop(db);
    // Act: decode every optional field through a newly opened store.
    let mut reopened = open_storage(&cache, &vectors).unwrap();
    let mut read = reopened.begin_read().unwrap();
    assert_eq!(
        read.read_node(&build_target("a.md#Section"))
            .unwrap()
            .unwrap()
            .metadata,
        knowledge
    );
    assert_eq!(
        read.read_node(&source_id).unwrap().unwrap().metadata,
        source
    );
}
#[test]
fn source_time_and_line_overflow_poison_the_unit() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let source = build_source("code.rs", None, u64::MAX);
    let mut knowledge = build_knowledge("a.md", "alpha");
    if let NodeMetadata::Knowledge(ref mut n) = knowledge {
        n.lines = letopis::domain::LineRange::new(1, u64::MAX).unwrap();
    }
    for node in [&source, &knowledge] {
        let mut write = db.begin_cache_write().unwrap();
        assert_eq!(
            write.upsert_node(node).unwrap_err().get_kind(),
            StorageErrorKind::InvalidRequest
        );
        assert!(write.commit().is_err());
    }
    assert_empty(&mut db);
}
#[test]
fn deleting_node_removes_fts_and_refuses_remaining_outgoing_edges() {
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let id = KnowledgeId::parse("a.md").unwrap();
    let mut write = db.begin_cache_write().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.replace_text(&id, "alpha").unwrap();
    write
        .insert_edge(&build_edge("a.md", build_target("b.md"), "supports"))
        .unwrap();
    write.commit().unwrap();
    // Act: missing caller cleanup fails and rolls back the preceding FTS deletion.
    let mut write = db.begin_cache_write().unwrap();
    assert_eq!(
        write
            .delete_node(&build_target("a.md"))
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    assert_eq!(
        db.begin_read().unwrap().match_text("alpha").unwrap().len(),
        1
    );
    let mut write = db.begin_cache_write().unwrap();
    write.delete_outgoing(&id).unwrap();
    write.delete_node(&build_target("a.md")).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    let mut read = db.begin_read().unwrap();
    assert!(read.match_text("alpha").unwrap().is_empty());
    assert!(read.list_edges(None).unwrap().is_empty());
    assert!(read.read_node(&build_target("a.md")).unwrap().is_none());
}
