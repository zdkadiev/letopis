use super::{ProvenanceFailure, ScopedFixture, build_filtered_request, open_storage};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri, TreeOid},
    service::{CacheWrite, CommandErrorKind, NodeMetadata, Searcher, Storage, StorageRead},
};
use std::path::Path;

#[test]
fn revision_cache_keeps_demand_order_and_resets_between_searches() {
    // Given: two candidates share one exact basis/source/revision comparison.
    let fixture = ScopedFixture::new();
    add_second_candidate(fixture.root.path(), 'a');
    let mut search = fixture.build_search(None, None);
    // When: the same service executes two searches.
    let first = search.search(&build_filtered_request()).unwrap();
    fixture.reads.set(0);
    let second = search.search(&build_filtered_request()).unwrap();
    // Then: comparison is shared only within each search; every live binding is observed.
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(
        *search.provenance.events.borrow(),
        [
            "validate", "compare", "observe", "observe", "validate", "compare", "observe",
            "observe"
        ]
    );
}

#[test]
fn first_observation_failure_precedes_later_candidate_comparison() {
    // Given: two candidates and a first-observation failure.
    let fixture = ScopedFixture::new();
    add_second_candidate(fixture.root.path(), 'b');
    let mut search = fixture.build_search(None, Some(ProvenanceFailure::Observation));
    // When: the first candidate cannot be observed.
    let error = search.search(&build_filtered_request()).unwrap_err();
    // Then: no later candidate comparison or observation is demanded.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(
        *search.provenance.events.borrow(),
        ["validate", "compare", "observe"]
    );
}

fn add_second_candidate(root: &Path, basis: char) {
    let mut storage = open_storage(root);
    let mut read = storage.begin_read_tx().unwrap();
    let stored = read
        .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        .unwrap()
        .unwrap();
    drop(read);
    let NodeMetadata::Knowledge(mut node) = stored.metadata else {
        panic!("knowledge fixture")
    };
    let id = KnowledgeId::parse("b.md").unwrap();
    node.source_snapshot = Some(TreeOid::parse(&basis.to_string().repeat(40)).unwrap());
    node.id = id.clone();
    node.uri = id.clone();
    let mut write = storage.begin_cache_write_tx().unwrap();
    write.upsert_node(&NodeMetadata::Knowledge(node)).unwrap();
    write.replace_text(&id, "query").unwrap();
    write
        .insert_edge(
            &Edge::new(
                id,
                EdgeTarget::Source(SourceUri::parse("old.rs").unwrap()),
                EdgeType::parse("external").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    write.commit().unwrap();
}
