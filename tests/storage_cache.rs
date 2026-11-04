#[path = "support/storage.rs"]
mod storage_support;
use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    open_storage,
    service::{ElementAddress, ReadTransaction, Storage, VectorOutcome},
};
use storage_support::{build_file, build_knowledge, build_source};
use tempfile::TempDir;
#[test]
fn boxed_read_keeps_graph_and_storage_contracts() {
    // Given: one knowledge node published at revision one.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.commit().unwrap();
    // When: the reader is used through the combined transaction interface.
    let mut read: Box<dyn ReadTransaction + '_> = storage.begin_read_tx().unwrap();
    let nodes = read.get_nodes().unwrap();
    let reference = read
        .get_node_ref_by_id(&ElementAddress::parse("a.md").unwrap())
        .unwrap()
        .unwrap();
    // Then: storage metadata and graph identity share the same pinned publication.
    assert_eq!(read.get_revision(), 1);
    assert_eq!(nodes.len(), 1);
    assert!(
        matches!(&nodes[0].metadata, NodeMetadata::Knowledge(node) if node.id.as_str() == "a.md")
    );
    assert_eq!(reference.element.as_str(), "a.md");
}

#[test]
fn boxed_storage_commit_drop_and_identity_guards() {
    // Given: storage accessed through an object-safe application interface.
    let root = TempDir::new().unwrap();
    let mut concrete = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let storage: &mut dyn Storage = &mut concrete;
    let mut committed = storage.begin_cache_write_tx().unwrap();
    committed
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    // When / Then: commit publishes exactly revision one.
    assert_eq!(committed.commit().unwrap(), 1);
    let mut abandoned = storage.begin_cache_write_tx().unwrap();
    abandoned
        .upsert_node(&build_knowledge("b.md", "beta"))
        .unwrap();
    drop(abandoned);
    // Then: dropping a writer leaves only the committed node and revision.
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 1);
    let nodes = read.get_nodes().unwrap();
    assert_eq!(nodes.len(), 1);
    assert!(
        matches!(&nodes[0].metadata, NodeMetadata::Knowledge(node) if node.id.as_str() == "a.md")
    );
    assert!(
        read.get_node_by_id(&build_target("b.md"))
            .unwrap()
            .is_none()
    );
    drop(read);
    // Given: a current vector configuration and a different requested identity.
    let current = Sha256::digest(b"model-a");
    let other = Sha256::digest(b"model-b");
    storage.reset_vectors(None, &current).unwrap();
    // When / Then: an obsolete caller receives no writer.
    assert!(matches!(
        storage.begin_vector_write_tx(&other).unwrap(),
        VectorOutcome::ConfigChanged
    ));
}

#[test]
fn cache_commit_preserves_node_row_id() {
    // Given: an inserted knowledge node with an initial FTS entry.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    let first = write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    // When / Then: successive publications replace searchable text while preserving the node row ID.
    assert_eq!(write.commit().unwrap(), 1);
    let mut write = storage.begin_cache_write_tx().unwrap();
    let second = write.upsert_node(&build_knowledge("a.md", "beta")).unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "beta")
        .unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(second, first);
    assert_eq!(
        read.get_text_matches_by_expression("alpha").unwrap().len(),
        0
    );
    assert_eq!(
        read.get_text_matches_by_expression("beta").unwrap().len(),
        1
    );
    assert_eq!(
        read.get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
            .unwrap()
            .unwrap()
            .row_id,
        first
    );
}

use letopis::domain::{Edge, EdgeType, ElementKind, LineRange, RepoPath, Sha256, TreeOid};
use letopis::service::{NodeMetadata, StorageErrorKind, StoredNode};
use rusqlite::Connection;
use std::error::Error;
#[test]
fn source_missing_bytes_and_declared_targets_round_trip() {
    // Given: source metadata with missing bytes and declared targets.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let source = build_source("code.rs#L2-L4", None, 3);
    let NodeMetadata::Source(ref meta) = source else {
        panic!()
    };
    let id = EdgeTarget::Source(meta.uri.clone());
    let external = build_edge("a.md", id.clone(), "external");
    let dangling = build_edge("a.md", build_target("missing.md"), "supports");
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_node(&source).unwrap();
    write.insert_edge(&external).unwrap();
    write.insert_edge(&dangling).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    // When / Then: the metadata is published and retrieved; source identities and optional data round-trip unchanged.
    assert_eq!(read.get_node_by_id(&id).unwrap().unwrap().metadata, source);
    assert_eq!(
        read.get_edges_by_source_id(None).unwrap(),
        vec![external, dangling]
    );
    assert!(
        read.get_node_by_id(&build_target("missing.md"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn incoming_edges_survive_target_delete_and_reappear() {
    // Given: incoming edges whose target can be deleted.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let declared = build_edge("a.md", build_target("b.md"), "supports");
    let mut write = db.begin_cache_write_tx().unwrap();
    for id in ["a.md", "b.md"] {
        write.upsert_node(&build_knowledge(id, "alpha")).unwrap();
    }
    write.insert_edge(&declared).unwrap();
    write.commit().unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("b.md").unwrap())
        .unwrap();
    write.delete_node(&build_target("b.md")).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    // When / Then: the target is deleted and recreated; incoming declarations survive and become active again.
    assert_eq!(
        read.get_edges_by_source_id(None).unwrap(),
        vec![declared.clone()]
    );
    assert!(
        read.get_node_by_id(&build_target("b.md"))
            .unwrap()
            .is_none()
    );
    drop(read);
    let mut write = db.begin_cache_write_tx().unwrap();
    write.upsert_node(&build_knowledge("b.md", "beta")).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    assert!(
        read.get_node_by_id(&build_target("b.md"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        read.get_edges_by_source_id(Some(&KnowledgeId::parse("a.md").unwrap()))
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
    // Given: two referring nodes; caller supplies their minimum creation time.
    let mut write = db.begin_cache_write_tx().unwrap();
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
    // When: removing the oldest referrer recomputes the projection in the same unit.
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("a.md").unwrap())
        .unwrap();
    write.delete_node(&build_target("a.md")).unwrap();
    write.upsert_node(&updated).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    // Then: stored source data follows the published projection.
    assert_eq!(read.get_node_by_id(&id).unwrap().unwrap().metadata, updated);
    assert_eq!(
        read.get_edges_by_source_id(None).unwrap(),
        vec![build_edge("b.md", id.clone(), "external")]
    );
    drop(read);
    // When / Then: caller removes the last reference and source together.
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("b.md").unwrap())
        .unwrap();
    write.delete_node(&id).unwrap();
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    // Then: stored source data follows the published projection.
    assert!(read.get_node_by_id(&id).unwrap().is_none());
    assert!(read.get_edges_by_source_id(None).unwrap().is_empty());
}
#[test]
fn valid_invalid_and_unmarked_file_rows_round_trip() {
    // Given: valid, invalid and unmarked file states.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let valid = build_file("a.md", "alpha");
    let mut invalid = build_file("b.md", "beta");
    invalid.valid = false;
    invalid.error = Some("invalid metadata".into());
    let unmarked = build_file("unmarked.md", "plain");
    let mut write = db.begin_cache_write_tx().unwrap();
    for row in [&valid, &invalid, &unmarked] {
        write.upsert_file(row).unwrap();
    }
    write.commit().unwrap();
    let mut read = db.begin_read_tx().unwrap();
    // When / Then: file rows are published and retrieved; each file state round-trips without changing its meaning.
    for row in [&valid, &invalid, &unmarked] {
        // When / Then: file rows are published and retrieved; each file state round-trips without changing its meaning.
        assert_eq!(
            read.get_file_by_path(&row.path).unwrap().as_ref(),
            Some(row)
        );
    }
    drop(read);
    let mut write = db.begin_cache_write_tx().unwrap();
    write.delete_file(&valid.path).unwrap();
    write.commit().unwrap();
    assert!(
        db.begin_read_tx()
            .unwrap()
            .get_file_by_path(&valid.path)
            .unwrap()
            .is_none()
    );
}
#[test]
fn caught_mutation_error_poisoned_unit_cannot_commit() {
    // Given: a cache write with a failing mutation.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    // When / Then: the caller catches the error and tries to commit; the poisoned unit cannot publish prior writes.
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
#[test]
fn dropped_unit_rolls_back() {
    // Given: an uncommitted cache publication.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    {
        let mut write = db.begin_cache_write_tx().unwrap();
        write
            .upsert_node(&build_knowledge("a.md", "alpha"))
            .unwrap();
        write
            .replace_text(&KnowledgeId::parse("a.md").unwrap(), "alpha")
            .unwrap();
        write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    }
    // When / Then: the write capability is dropped; the transaction rolls back.
    assert_empty(&mut db);
}
#[test]
fn late_fts_error_rolls_back_all_changes() {
    // Given: a cache write with prior valid mutations.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    // When / Then: a later FTS operation fails; the entire publication rolls back.
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
    // Given: unsigned values beyond SQLite's supported range.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut node = build_knowledge("a.md", "alpha");
    let NodeMetadata::Knowledge(ref mut meta) = node else {
        panic!()
    };
    meta.created_at = u64::MAX;
    let mut write = db.begin_cache_write_tx().unwrap();
    // When / Then: the values are passed to storage; values are rejected without wrapping.
    assert!(write.upsert_node(&node).is_err());
    assert!(write.commit().is_err());
    for field in ["size", "inode"] {
        let mut file = build_file("a.md", "alpha");
        if field == "size" {
            file.size = u64::MAX;
        } else {
            file.inode = u64::MAX;
        }
        let mut write = db.begin_cache_write_tx().unwrap();
        // When / Then: the values are passed to storage; values are rejected without wrapping.
        assert!(write.upsert_file(&file).is_err());
        assert!(write.commit().is_err());
    }
    assert_empty(&mut db);
}
#[test]
fn revision_overflow_rolls_back() {
    // Given: a stored publication revision at its maximum.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    Connection::open(&cache)
        .unwrap()
        .execute("update project set revision=?1", [i64::MAX])
        .unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    // When / Then: another publication is committed; revision overflow rolls back all mutations.
    assert!(write.commit().is_err());
    let mut read = db.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), i64::MAX as u64);
    assert!(
        read.get_node_by_id(&build_target("a.md"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn duplicate_edges_and_invalid_checks_are_rejected() {
    // Given: duplicate edges and inconsistent node metadata.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    let e = build_edge("a.md", build_target("b.md"), "supports");
    write.insert_edge(&e).unwrap();
    // When / Then: the invalid mutations are attempted; storage rejects them without publishing partial state.
    assert_eq!(
        write.insert_edge(&e).unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    let sql = Connection::open(&cache).unwrap();
    for query in [
        "insert into nodes(kind,uri,created_at,source_snapshot) values('source','bad',1,'snapshot')",
        "insert into nodes(kind,uri,created_at,search_hash) values('source','bad',1,'hash')",
        "insert into nodes(kind,uri,created_at,line_start) values('source','bad',1,1)",
        "insert into nodes(kind,uri,created_at) values('vertex','bad',1)",
        "insert into nodes(kind,uri,created_at,line_start,line_end) values('source','bad',1,0,1)",
        "insert into files values('x',1,2,3,'hash',4,1,'error')",
        "insert into files values('x',1,2,3,'hash',4,0,null)",
        "insert into files values('x',1,2,3,'hash',4,2,null)",
    ] {
        // When / Then: the invalid mutations are attempted; storage rejects them without publishing partial state.
        assert!(sql.execute(query, []).is_err());
    }
}
#[test]
fn contentless_fts_does_not_return_original_text() {
    // Given: a published private search token in a contentless FTS index.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "privatepayload"))
        .unwrap();
    write
        .replace_text(&KnowledgeId::parse("a.md").unwrap(), "privatepayload")
        .unwrap();
    write.commit().unwrap();
    let sql = Connection::open(&cache).unwrap();
    // When / Then: direct SQL returns no original text while indexed matching remains available.
    assert_eq!(
        sql.query_row("select text from text_index", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    let columns: Vec<String>=sql.prepare("select name from pragma_table_info('nodes') union all select name from pragma_table_info('files')").unwrap().query_map([],|r|r.get(0)).unwrap().map(Result::unwrap).collect();
    assert!(
        !columns
            .iter()
            .any(|c| ["text", "body", "content"].contains(&c.as_str()))
    );
    let mut read = db.begin_read_tx().unwrap();
    assert_eq!(
        read.get_text_matches_by_expression("privatepayload")
            .unwrap()
            .len(),
        1
    );
    let expression = "privatepayload Authorization:Bearer-secret \"";
    let error = read.get_text_matches_by_expression(expression).unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(error.source().is_some());
    for marker in ["privatepayload", "Authorization", expression] {
        // When / Then: malformed-query diagnostics expose none of the private input markers.
        assert!(!error.to_string().contains(marker));
        assert!(!error.get_message().contains(marker));
    }
}

#[test]
fn empty_cache_publications_preserve_vector_identity_and_state() {
    // Given: vector identity and state before an empty cache write.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let vectors = root.path().join("v");
    let mut db = open_storage(&cache, &vectors).unwrap();
    let sql = Connection::open(&vectors).unwrap();
    sql.execute("insert into meta values(1,?1)", ["a".repeat(64)])
        .unwrap();
    sql.execute(
        "insert into embed_failures values(?1,2,'safe')",
        ["b".repeat(64)],
    )
    .unwrap();
    // When: even an empty explicitly committed unit advances only the cache revision.
    // Then: vector identity and state remain unchanged.
    assert_eq!(db.begin_cache_write_tx().unwrap().commit().unwrap(), 1);
    assert_eq!(db.begin_cache_write_tx().unwrap().commit().unwrap(), 2);
    let read = db.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 2);
    assert_eq!(
        read.get_config_identity(),
        Some(Sha256::parse(&"a".repeat(64)).unwrap())
    );
    assert_eq!(
        sql.query_row("select attempts from embed_failures", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
#[test]
fn fts_matches_use_native_score_then_id_without_candidate_limit() {
    // Given: equal-score documents and one stronger lexical match.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
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
    // When: all lexical matches are retrieved without a candidate limit.
    let results = db
        .begin_read_tx()
        .unwrap()
        .get_text_matches_by_expression("alpha")
        .unwrap();
    // Then: all matches sort by native score and then normalized identity.
    assert_eq!(
        results.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["strong.md", "a.md", "b.md", "z.md"]
    );
    assert!(results[0].bm25 < results[1].bm25);
}
#[test]
fn malformed_persisted_values_return_internal_errors() {
    // Given: malformed persisted metadata values.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let mut db = open_storage(&cache, &root.path().join("v")).unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.upsert_file(&build_file("a.md", "alpha")).unwrap();
    write.commit().unwrap();
    let sql = Connection::open(&cache).unwrap();
    // When / Then: typed storage reads decode the rows; malformed values return internal errors.
    for update in [
        "update nodes set hash='invalid'",
        "update nodes set hash='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',created_at=-1",
    ] {
        sql.execute(update, []).unwrap();
        // When / Then: typed storage reads decode the rows; malformed values return internal errors.
        assert_eq!(
            db.begin_read_tx()
                .unwrap()
                .get_node_by_id(&build_target("a.md"))
                .unwrap_err()
                .get_kind(),
            StorageErrorKind::InternalError
        );
    }
    sql.execute("update files set inode=-1", []).unwrap();
    assert_eq!(
        db.begin_read_tx()
            .unwrap()
            .get_file_by_path(&RepoPath::parse("a.md").unwrap())
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InternalError
    );
    sql.execute(
        "insert into unresolved_edges select id,'b.md','INVALID' from nodes where uri='a.md'",
        [],
    )
    .unwrap();
    assert_eq!(
        db.begin_read_tx()
            .unwrap()
            .get_edges_by_source_id(None)
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InternalError
    );
}

#[test]
fn knowledge_and_source_optional_fields_survive_reopen() {
    // Given: knowledge and source metadata with optional fields.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("c");
    let vectors = root.path().join("v");
    let mut knowledge = build_knowledge("a.md#Section", "alpha");
    if let NodeMetadata::Knowledge(ref mut n) = knowledge {
        n.uri = KnowledgeId::parse("a.md#Section").unwrap();
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
    let mut write = db.begin_cache_write_tx().unwrap();
    write.upsert_node(&knowledge).unwrap();
    write.upsert_node(&source).unwrap();
    write.commit().unwrap();
    drop(db);
    // When: decode every optional field through a newly opened store.
    let mut reopened = open_storage(&cache, &vectors).unwrap();
    let mut read = reopened.begin_read_tx().unwrap();
    // Then: optional metadata fields survive intact.
    assert_eq!(
        read.get_node_by_id(&build_target("a.md#Section"))
            .unwrap()
            .unwrap()
            .metadata,
        knowledge
    );
    assert_eq!(
        read.get_node_by_id(&source_id).unwrap().unwrap().metadata,
        source
    );
}
#[test]
fn source_time_and_line_overflow_poison_the_unit() {
    // Given: source timestamps or line bounds beyond the supported range.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let source = build_source("code.rs", None, u64::MAX);
    let mut knowledge = build_knowledge("a.md", "alpha");
    if let NodeMetadata::Knowledge(ref mut n) = knowledge {
        n.lines = letopis::domain::LineRange::new(1, u64::MAX).unwrap();
    }
    // When / Then: the source metadata is written; the write unit is poisoned without partial data.
    for node in [&source, &knowledge] {
        let mut write = db.begin_cache_write_tx().unwrap();
        // When / Then: the source metadata is written; the write unit is poisoned without partial data.
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
    // Given: knowledge with searchable text and an outgoing edge.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let id = KnowledgeId::parse("a.md").unwrap();
    let mut write = db.begin_cache_write_tx().unwrap();
    write
        .upsert_node(&build_knowledge("a.md", "alpha"))
        .unwrap();
    write.replace_text(&id, "alpha").unwrap();
    write
        .insert_edge(&build_edge("a.md", build_target("b.md"), "supports"))
        .unwrap();
    write.commit().unwrap();
    // When: missing caller cleanup fails and rolls back the preceding FTS deletion.
    let mut write = db.begin_cache_write_tx().unwrap();
    // Then: failed deletion retains the node, edge and searchable text.
    assert_eq!(
        write
            .delete_node(&build_target("a.md"))
            .unwrap_err()
            .get_kind(),
        StorageErrorKind::InvalidRequest
    );
    assert!(write.commit().is_err());
    assert_eq!(
        db.begin_read_tx()
            .unwrap()
            .get_text_matches_by_expression("alpha")
            .unwrap()
            .len(),
        1
    );
    let mut write = db.begin_cache_write_tx().unwrap();
    write.delete_outgoing(&id).unwrap();
    write.delete_node(&build_target("a.md")).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    let mut read = db.begin_read_tx().unwrap();
    assert!(
        read.get_text_matches_by_expression("alpha")
            .unwrap()
            .is_empty()
    );
    assert!(read.get_edges_by_source_id(None).unwrap().is_empty());
    assert!(
        read.get_node_by_id(&build_target("a.md"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn get_edges_keeps_optional_filter_and_order() {
    // Given: edges with distinct owners and deterministic ordering.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let first = build_edge("a.md", build_target("x.md"), "refutes");
    let second = build_edge("a.md", build_target("z.md"), "supports");
    let third = build_edge("b.md", build_target("a.md"), "supports");
    let mut write = db.begin_cache_write_tx().unwrap();
    for id in ["a.md", "b.md"] {
        write.upsert_node(&build_knowledge(id, "alpha")).unwrap();
    }
    for edge in [&third, &second, &first] {
        write.insert_edge(edge).unwrap();
    }
    write.commit().unwrap();
    // When: all edges and one owner's edges are retrieved.
    let mut read = db.begin_read_tx().unwrap();
    let all = read.get_edges_by_source_id(None).unwrap();
    let selected = read
        .get_edges_by_source_id(Some(&KnowledgeId::parse("a.md").unwrap()))
        .unwrap();
    let absent = read
        .get_edges_by_source_id(Some(&KnowledgeId::parse("absent.md").unwrap()))
        .unwrap();
    // Then: the optional filter preserves deterministic order.
    assert_eq!(all, vec![first.clone(), second.clone(), third]);
    assert_eq!(selected, vec![first, second]);
    assert!(absent.is_empty());
}

#[test]
fn file_text_values_remain_literal() {
    // Given: file text containing punctuation and SQL-like bytes.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut file = build_file("notes/é'--.md", "alpha");
    file.valid = false;
    file.error = Some("safe 'quoted' ?1 -- mémoire".into());
    let untouched = build_file("other.md", "beta");
    let mut write = db.begin_cache_write_tx().unwrap();
    write.upsert_file(&file).unwrap();
    write.upsert_file(&untouched).unwrap();
    write.commit().unwrap();
    // When: read the literal text.
    let mut read = db.begin_read_tx().unwrap();
    // Then: bound text stays literal and file state changes atomically.
    assert_eq!(
        read.get_file_by_path(&file.path).unwrap(),
        Some(file.clone())
    );
    drop(read);
    // Given: replace the existing row.
    file.error = None;
    file.valid = true;
    let mut write = db.begin_cache_write_tx().unwrap();
    write.upsert_file(&file).unwrap();
    write.commit().unwrap();
    // When: file rows are retrieved after update or deletion.
    let mut read = db.begin_read_tx().unwrap();
    // Then: bound text stays literal and file state changes atomically.
    assert_eq!(
        read.get_file_by_path(&file.path).unwrap(),
        Some(file.clone())
    );
    drop(read);
    // Given: delete only the selected row.
    let mut write = db.begin_cache_write_tx().unwrap();
    write.delete_file(&file.path).unwrap();
    write.commit().unwrap();
    // When: file rows are retrieved after update or deletion.
    let mut read = db.begin_read_tx().unwrap();
    // Then: bound text stays literal and file state changes atomically.
    assert!(read.get_file_by_path(&file.path).unwrap().is_none());
    assert_eq!(
        read.get_file_by_path(&untouched.path).unwrap(),
        Some(untouched)
    );
}

#[test]
fn get_files_preserves_exact_paths_and_state() {
    // Given: marked, invalid and unmarked files with literal SQL wildcards.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let marked = build_file("a_b.md", "alpha");
    let mut invalid = build_file("a%b.md", "broken");
    invalid.mtime_ns = -23;
    invalid.inode = 29;
    invalid.checked_at_ns = 31;
    invalid.valid = false;
    invalid.error = Some("invalid metadata".into());
    let unmarked = build_file("nested/a.markdown", "plain text");
    let mut write = db.begin_cache_write_tx().unwrap();
    for row in [&unmarked, &marked, &invalid] {
        write.upsert_file(row).unwrap();
    }
    write
        .upsert_node(&build_knowledge("a_b.md", "alpha"))
        .unwrap();
    write.commit().unwrap();
    let expected_file_rows = vec![invalid, marked, unmarked];

    // When: all file rows are retrieved.
    let rows = db.begin_read_tx().unwrap().get_files().unwrap();

    // Then: every persisted field and exact deterministic path ordering.
    assert_eq!(
        rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
        ["a%b.md", "a_b.md", "nested/a.markdown"]
    );
    assert_eq!(rows, expected_file_rows);
}

#[test]
fn get_nodes_preserves_metadata_and_row_ids() {
    // Given: knowledge and sources are inserted out of identity order.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let source = build_source("code.rs#L2-L4", Some(Sha256::digest(b"source")), 3);
    let missing = build_source("missing.rs", None, 11);
    let anchored = build_knowledge("a_b.md#part", "anchored");
    let mut write = db.begin_cache_write_tx().unwrap();
    let mut expected = Vec::new();
    for metadata in [&missing, &source, &anchored] {
        expected.push(StoredNode {
            row_id: write.upsert_node(metadata).unwrap(),
            metadata: metadata.clone(),
        });
    }
    write
        .upsert_node(&build_knowledge("a%b.md", "old"))
        .unwrap();
    write.commit().unwrap();
    let original = db
        .begin_read_tx()
        .unwrap()
        .get_node_by_id(&build_target("a%b.md"))
        .unwrap()
        .unwrap();
    let mut changed = build_knowledge("a%b.md", "updated");
    if let NodeMetadata::Knowledge(ref mut node) = changed {
        node.kind = ElementKind::Metavertex;
        node.created_at = 41;
        node.source_snapshot = Some(TreeOid::parse(&"a".repeat(40)).unwrap());
        node.lines = LineRange::new(2, 9).unwrap();
    }
    let mut write = db.begin_cache_write_tx().unwrap();
    write.upsert_node(&changed).unwrap();
    write.commit().unwrap();
    expected.reverse();
    expected.insert(
        0,
        StoredNode {
            row_id: original.row_id,
            metadata: changed,
        },
    );

    // When: all nodes are retrieved.
    let rows = db.begin_read_tx().unwrap().get_nodes().unwrap();

    // Then: complete metadata, identity order, and stable upsert row ID.
    let updated = &rows[0];
    assert_eq!(updated.row_id, original.row_id);
    assert_eq!(rows, expected);
}

#[test]
fn get_empty_published_state_returns_no_rows() {
    // Given: fresh storage with no published rows.
    let root = TempDir::new().unwrap();
    let mut db = open_storage(&root.path().join("c"), &root.path().join("v")).unwrap();
    let mut read = db.begin_read_tx().unwrap();

    // When: all file and node rows are retrieved.
    let files = read.get_files().unwrap();
    let nodes = read.get_nodes().unwrap();

    // Then: both collections are empty.
    assert!(files.is_empty());
    assert!(nodes.is_empty());
}

fn build_edge(source: &str, target: EdgeTarget, kind: &str) -> Edge {
    Edge::new(
        KnowledgeId::parse(source).unwrap(),
        target,
        EdgeType::parse(kind).unwrap(),
    )
    .unwrap()
}

fn assert_empty(db: &mut impl Storage) {
    let mut read = db.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 0);
    assert!(
        read.get_node_by_id(&build_target("a.md"))
            .unwrap()
            .is_none()
    );
    assert!(
        read.get_file_by_path(&RepoPath::parse("a.md").unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        read.get_text_matches_by_expression("alpha")
            .unwrap()
            .is_empty()
    );
}

fn build_target(id: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
}
