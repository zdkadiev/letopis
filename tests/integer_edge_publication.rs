mod support;

use letopis::{
    domain::KnowledgeId,
    service::{FileRow, NodeMetadata, Refresher, Storage},
    wire_document_refresher,
};
use rusqlite::{Connection, types::Value as SqlValue};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use support::reading::ReadingRepo;

const TARGET: &str = "# Target {#target}\n<!-- letopis: {\"created_at\":2} -->\nTargetToken\n";
const FORWARD: &str = "[target](z.md#target \"letopis:type=supports;path=z.md#target\")";
const SOURCES: &str = "[local](code.rs \"letopis:type=external;path=code.rs\") [remote](https://Host/x?q=1#part \"letopis:type=external;path=https://Host/x?q=1#part\")";

#[test]
fn new_targets_resolve_independently_of_file_order() {
    for incremental in [false, true] {
        // Given: an early owner points at a later anchor and two derived source nodes.
        let repo = ReadingRepo::new();
        let mut documents = wire_document_refresher(repo.get_root()).unwrap();
        if incremental {
            documents.refresh().unwrap();
        }
        repo.write_knowledge("a.md", &format!("OwnerToken {FORWARD} {SOURCES}"));
        repo.write("z.md", TARGET.as_bytes());
        repo.write("code.rs", b"source bytes\n");
        // When: reindex or one incremental refresh publishes the files together.
        let result = if incremental {
            documents.refresh().unwrap()
        } else {
            documents.reindex().unwrap()
        };
        // Then: raw integer endpoints and public URI endpoints describe the same three edges.
        assert_eq!(result.revision, if incremental { 2 } else { 1 });
        assert_eq!(
            get_raw_edges(&repo),
            [
                (1, 2, "supports".into()),
                (1, 3, "external".into()),
                (1, 4, "external".into())
            ]
        );
        assert_eq!(
            get_edges(&repo, None),
            json!([
                ["a.md", "code.rs", "external"],
                ["a.md", "https://Host/x?q=1#part", "external"],
                ["a.md", "z.md#target", "supports"]
            ])
        );
        assert_eq!(get_pending(&repo), Vec::<(i64, String, String)>::new());
        let source_basis = json!({"code":"validation_failed","message":"knowledge source snapshot is missing","details":{"path":"a.md","reason":"missing_source_snapshot","element_id":"a.md"}});
        assert_findings(&repo, &json!([source_basis]));
        assert_foreign_keys(&repo);
    }
}

#[test]
fn unchanged_owner_resolves_new_target_and_preserves_logical_union_order() {
    // Given: one unchanged owner has resolved and unresolved links; another owner has a relation.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        &format!("OwnerToken [known](b.md \"letopis:type=supports;path=b.md\") {FORWARD}"),
    );
    repo.write_knowledge("b.md", "KnownToken");
    repo.write_knowledge(
        "c.md",
        "[known](b.md \"letopis:type=related_to;path=b.md\")",
    );
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before_owner = get_file(&repo, "a.md");
    // When: the missing target is inspected in the first committed publication.
    let checked = invoke(&repo, "check", &json!({}));
    // Then: the logical union sorts both representations and source filtering excludes other owners.
    assert_eq!(checked["errors"], json!([build_dangling_problem()]));
    assert_eq!(
        get_edges(&repo, None),
        json!([
            ["a.md", "b.md", "supports"],
            ["a.md", "z.md#target", "supports"],
            ["c.md", "b.md", "related_to"]
        ])
    );
    assert_eq!(
        get_edges(&repo, Some("a.md")),
        json!([
            ["a.md", "b.md", "supports"],
            ["a.md", "z.md#target", "supports"]
        ])
    );
    assert_eq!(get_visible(&repo), ["b.md"]);
    assert!(!get_node_uris(&repo).contains(&"z.md#target".into()));
    assert_foreign_keys(&repo);

    // Given: target bytes appear, while status still observes the prior publication.
    repo.write("z.md", TARGET.as_bytes());
    assert_eq!(
        invoke(&repo, "status", &json!({}))["problems"],
        json!([build_dangling_problem()])
    );
    // When: only the new target is refreshed.
    assert_eq!(documents.refresh().unwrap().files_updated, 1);
    // Then: the unchanged owner resolves without losing its URI or file state.
    assert_findings(&repo, &json!([]));
    assert_eq!(get_visible(&repo), ["b.md", "z.md#target"]);
    assert_eq!(get_file(&repo, "a.md"), before_owner);
    assert!(get_pending(&repo).is_empty());
    assert_foreign_keys(&repo);
}

#[test]
fn unchanged_owner_tracks_target_deletion_and_return() {
    // Given: a published owner and its target have distinct stable identities.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", &format!("OwnerToken {FORWARD}"));
    repo.write("z.md", TARGET.as_bytes());
    let owner = fs::read(repo.get_root().join("a.md")).unwrap();
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    let before_owner = get_file(&repo, "a.md");
    let original_key = get_key(&repo, "z.md#target");
    // Given: deleting target bytes has not changed the observed publication yet.
    fs::remove_file(repo.get_root().join("z.md")).unwrap();
    assert_eq!(invoke(&repo, "status", &json!({}))["problems"], json!([]));
    // When: target removal is published.
    assert_eq!(documents.refresh().unwrap().files_removed, 1);
    // Then: the surviving declaration becomes the same diagnostic and is excluded from traversal.
    assert_findings(&repo, &json!([build_dangling_problem()]));
    assert!(get_visible(&repo).is_empty());
    assert!(!get_node_uris(&repo).contains(&"z.md#target".into()));
    assert_eq!(
        get_pending(&repo),
        [(
            get_key(&repo, "a.md"),
            "z.md#target".into(),
            "supports".into()
        )]
    );
    assert_foreign_keys(&repo);

    // Given: the target returns without any owner edit.
    repo.write("z.md", TARGET.as_bytes());
    // When: the restored node is published.
    assert_eq!(documents.refresh().unwrap().files_updated, 1);
    // Then: a new numeric key replaces the removed endpoint and every public finding disappears.
    let restored_key = get_key(&repo, "z.md#target");
    assert_ne!(restored_key, original_key);
    assert!(
        !get_raw_edges(&repo)
            .iter()
            .any(|(_, target, _)| *target == original_key)
    );
    assert!(get_raw_edges(&repo).contains(&(
        get_key(&repo, "a.md"),
        restored_key,
        "supports".into()
    )));
    assert_findings(&repo, &json!([]));
    assert_eq!(get_visible(&repo), ["z.md#target"]);
    assert_eq!(get_file(&repo, "a.md"), before_owner);
    assert_eq!(fs::read(repo.get_root().join("a.md")).unwrap(), owner);
    assert!(get_pending(&repo).is_empty());
    assert_foreign_keys(&repo);
}

#[test]
fn valid_owner_edit_and_delete_clear_both_declaration_representations() {
    // Given: a valid owner declares present, absent, local and remote targets.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        &format!(
            "OwnerToken {FORWARD} {SOURCES} [known](b.md \"letopis:type=supports;path=b.md\")"
        ),
    );
    repo.write_knowledge("b.md", "KnownToken");
    repo.write("code.rs", b"source\n");
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    assert_eq!(get_raw_edges(&repo).len(), 3);
    assert_eq!(get_pending(&repo).len(), 1);
    // When: a valid edit retains only the present knowledge relation.
    repo.write_knowledge(
        "a.md",
        "EditedToken [known](b.md \"letopis:type=supports;path=b.md\")",
    );
    documents.refresh().unwrap();
    // Then: obsolete pending declarations and unreferenced derived source nodes are gone.
    assert_eq!(get_node_uris(&repo), ["a.md", "b.md"]);
    assert_eq!(
        get_edges(&repo, None),
        json!([["a.md", "b.md", "supports"]])
    );
    assert!(get_pending(&repo).is_empty());
    assert_eq!(invoke(&repo, "check", &json!({}))["errors"], json!([]));
    assert_foreign_keys(&repo);
    // Given: the edited owner gains another absent declaration before it is removed.
    repo.write_knowledge(
        "a.md",
        &format!("EditedToken {FORWARD} [known](b.md \"letopis:type=supports;path=b.md\")"),
    );
    documents.refresh().unwrap();
    // When: deleting the owner clears both forms, then the previously absent target appears.
    fs::remove_file(repo.get_root().join("a.md")).unwrap();
    documents.refresh().unwrap();
    repo.write("z.md", TARGET.as_bytes());
    documents.refresh().unwrap();
    // Then: no old declaration can be promoted or leave stale FTS membership.
    assert_eq!(get_node_uris(&repo), ["b.md", "z.md#target"]);
    assert_eq!(get_edges(&repo, None), json!([]));
    assert!(get_pending(&repo).is_empty());
    assert_eq!(
        get_matches(&repo, "OwnerToken OR EditedToken"),
        Vec::<String>::new()
    );
    assert_foreign_keys(&repo);
}

#[test]
fn invalid_owner_keeps_last_valid_declarations_as_targets_change() {
    // Given: an owner first publishes a last-valid declaration to an absent target.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", &format!("LastValidToken {FORWARD}"));
    let mut documents = wire_document_refresher(repo.get_root()).unwrap();
    documents.refresh().unwrap();
    // When: invalid Markdown is published, then its target appears independently.
    repo.write("a.md", b"<!-- letopis: invalid -->\nInvalidToken\n");
    assert_eq!(documents.refresh().unwrap().invalid_files_updated, 1);
    let invalid_file = get_file(&repo, "a.md");
    repo.write("z.md", TARGET.as_bytes());
    assert_eq!(documents.refresh().unwrap().files_updated, 1);
    // Then: last-valid declarations resolve while the stored invalid-file finding survives.
    assert_findings(&repo, &json!([build_invalid_owner_problem()]));
    assert_eq!(get_matches(&repo, "LastValidToken"), ["a.md"]);
    assert!(get_matches(&repo, "InvalidToken").is_empty());
    assert_eq!(get_visible(&repo), ["z.md#target"]);
    assert_eq!(get_file(&repo, "a.md"), invalid_file);
    assert!(get_pending(&repo).is_empty());
    assert_foreign_keys(&repo);
    // When: the target is deleted without changing the invalid owner again.
    fs::remove_file(repo.get_root().join("z.md")).unwrap();
    documents.refresh().unwrap();
    // Then: the retained declaration becomes dangling alongside the unchanged file finding.
    assert_findings(
        &repo,
        &json!([build_dangling_problem(), build_invalid_owner_problem()]),
    );
    assert_eq!(get_file(&repo, "a.md"), invalid_file);
    assert_foreign_keys(&repo);
}

#[test]
fn faults_after_nodes_demotion_and_promotion_restore_the_whole_publication() {
    for cut in ["nodes", "demotion", "promotion"] {
        // Given: a complete old publication and a SQL fault at a later mutation boundary.
        let repo = ReadingRepo::new();
        repo.write_knowledge(
            "a.md",
            if cut == "nodes" {
                "OwnerToken"
            } else {
                FORWARD
            },
        );
        repo.write_knowledge("c.md", "AlphaToken");
        if cut != "promotion" {
            repo.write("z.md", TARGET.as_bytes());
        }
        let mut documents = wire_document_refresher(repo.get_root()).unwrap();
        documents.refresh().unwrap();
        let before = get_snapshot(&repo);
        let db = get_db(&repo);
        db.execute_batch(match cut {
            "nodes" => "create trigger deny_publication before insert on edges begin select raise(abort, 'fault after nodes'); end;",
            "demotion" => "create trigger deny_publication before delete on nodes when old.uri='z.md#target' begin select raise(abort, 'fault after demotion'); end;",
            "promotion" => "create trigger deny_publication before update on project begin select raise(abort, 'fault after promotion'); end;",
            _ => unreachable!(),
        }).unwrap();
        if cut == "nodes" {
            repo.write_knowledge("a.md", &format!("ReplacementToken {FORWARD}"));
        }
        if cut == "demotion" {
            fs::remove_file(repo.get_root().join("z.md")).unwrap();
        } else {
            repo.write_knowledge("c.md", "BravoToken");
            repo.write("z.md", TARGET.as_bytes());
        }
        // When: refresh reaches the injected failure after earlier writes in its transaction.
        assert!(documents.refresh().is_err(), "missing fault at {cut}");
        // Then: a fresh connection sees exactly the old files, node keys, FTS and declarations.
        assert_eq!(get_snapshot(&repo), before, "partial publication at {cut}");
        assert_foreign_keys(&repo);
        // When: the fault is removed and the same delta is retried.
        db.execute_batch("drop trigger deny_publication").unwrap();
        assert_eq!(documents.refresh().unwrap().revision, 2);
        // Then: the retry publishes one complete target transition.
        assert_eq!(get_pending(&repo).len(), usize::from(cut == "demotion"));
        assert_eq!(get_raw_edges(&repo).len(), usize::from(cut != "demotion"));
        assert_foreign_keys(&repo);
    }
}

fn assert_findings(repo: &ReadingRepo, expected: &Value) {
    assert_eq!(invoke(repo, "check", &json!({}))["errors"], *expected);
    assert_eq!(invoke(repo, "status", &json!({}))["problems"], *expected);
}

fn build_dangling_problem() -> Value {
    json!({"code":"validation_failed","message":"knowledge target is missing","details":{"path":"a.md","reason":"dangling_target","element_id":"a.md","target":"z.md#target"}})
}

fn build_invalid_owner_problem() -> Value {
    json!({"code":"validation_failed","message":"invalid letopis metadata","details":{"path":"a.md","reason":"invalid_metadata"}})
}

fn get_visible(repo: &ReadingRepo) -> Vec<String> {
    invoke(
        repo,
        "traverse",
        &json!({"element_ids":["a.md"],"mode":"both","direction":"outgoing","depth":1,"limit":10}),
    )["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|element| element["element_id"].as_str().unwrap().to_owned())
        .collect()
}

fn get_edges(repo: &ReadingRepo, source: Option<&str>) -> Value {
    let mut storage = repo.open_storage();
    let id = source.map(|id| KnowledgeId::parse(id).unwrap());
    let triples: Vec<_> = storage
        .begin_read_tx()
        .unwrap()
        .get_edges_by_source_id(id.as_ref())
        .unwrap()
        .into_iter()
        .map(|edge| {
            (
                edge.get_source().as_str().to_owned(),
                edge.get_target().as_str().to_owned(),
                edge.get_edge_type().as_str().to_owned(),
            )
        })
        .collect();
    json!(triples)
}

fn get_node_uris(repo: &ReadingRepo) -> Vec<String> {
    repo.open_storage()
        .begin_read_tx()
        .unwrap()
        .get_nodes()
        .unwrap()
        .into_iter()
        .map(|node| match node.metadata {
            NodeMetadata::Knowledge(node) => node.uri.as_str().into(),
            NodeMetadata::Source(node) => node.uri.as_str().into(),
        })
        .collect()
}

fn get_file(repo: &ReadingRepo, path: &str) -> FileRow {
    repo.open_storage()
        .begin_read_tx()
        .unwrap()
        .get_files()
        .unwrap()
        .into_iter()
        .find(|file| file.path.as_str() == path)
        .unwrap()
}

fn get_matches(repo: &ReadingRepo, expression: &str) -> Vec<String> {
    repo.open_storage()
        .begin_read_tx()
        .unwrap()
        .get_text_matches_by_expression(expression)
        .unwrap()
        .into_iter()
        .map(|entry| entry.id.as_str().to_owned())
        .collect()
}

fn get_raw_edges(repo: &ReadingRepo) -> Vec<(i64, i64, String)> {
    get_db(repo)
        .prepare("select source_id,target_id,type from edges order by source_id,target_id,type")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn get_pending(repo: &ReadingRepo) -> Vec<(i64, String, String)> {
    let db = get_db(repo);
    let mut query = db.prepare("select source_id,target_uri,type from unresolved_edges order by source_id,target_uri,type").unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn get_key(repo: &ReadingRepo, uri: &str) -> i64 {
    get_db(repo)
        .query_row("select id from nodes where uri=?1", [uri], |row| row.get(0))
        .unwrap()
}

fn assert_foreign_keys(repo: &ReadingRepo) {
    assert!(
        !get_db(repo)
            .prepare("pragma foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

fn get_snapshot(repo: &ReadingRepo) -> BTreeMap<&'static str, Vec<Vec<SqlValue>>> {
    let db = get_db(repo);
    db.execute_batch("begin").unwrap();
    [
        "project",
        "nodes",
        "files",
        "edges",
        "unresolved_edges",
        "sqlite_sequence",
        "text_index_data",
        "text_index_idx",
        "text_index_docsize",
        "text_index_config",
    ]
    .into_iter()
    .map(|table| {
        let mut query = db
            .prepare(&format!("select * from {table} order by 1"))
            .unwrap();
        let columns = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get(column))
                    .collect::<Result<Vec<SqlValue>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (table, rows)
    })
    .collect()
}

fn invoke(repo: &ReadingRepo, command: &str, input: &Value) -> Value {
    let output = repo.invoke(&[command], input.to_string().as_bytes());
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    value["result"].clone()
}

fn get_db(repo: &ReadingRepo) -> Connection {
    Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap()
}
