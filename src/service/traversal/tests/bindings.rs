use super::super::observe_bindings;
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    service::{
        DocumentError, DocumentErrorKind, DocumentResult, ElementAddress, GraphNeighbor,
        GraphNodeKey, GraphNodeRef, GraphRead, KnowledgeMetadata, NeighborPageRequest,
        ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, StorageResult,
    },
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

const EXPECTED_BINDINGS: [&str; 70] = [
    "s000.rs", "s001.rs", "s002.rs", "s003.rs", "s004.rs", "s005.rs", "s006.rs", "s007.rs",
    "s008.rs", "s009.rs", "s010.rs", "s011.rs", "s012.rs", "s013.rs", "s014.rs", "s015.rs",
    "s016.rs", "s017.rs", "s018.rs", "s019.rs", "s020.rs", "s021.rs", "s022.rs", "s023.rs",
    "s024.rs", "s025.rs", "s026.rs", "s027.rs", "s028.rs", "s029.rs", "s030.rs", "s031.rs",
    "s032.rs", "s033.rs", "s034.rs", "s035.rs", "s036.rs", "s037.rs", "s038.rs", "s039.rs",
    "s040.rs", "s041.rs", "s042.rs", "s043.rs", "s044.rs", "s045.rs", "s046.rs", "s047.rs",
    "s048.rs", "s049.rs", "s050.rs", "s051.rs", "s052.rs", "s053.rs", "s054.rs", "s055.rs",
    "s056.rs", "s057.rs", "s058.rs", "s059.rs", "s060.rs", "s061.rs", "s062.rs", "s063.rs",
    "s064.rs", "s065.rs", "s066.rs", "s067.rs", "s068.rs", "s069.rs",
];

#[test]
fn observes_large_source_bindings_pagewise() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
        observed: RefCell::new(Vec::new()),
    };
    let node = build_knowledge();
    // When
    let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
    // Then
    assert_eq!(
        *events.borrow(),
        [
            "resolve",
            "page:0",
            "observe:32",
            "page:32",
            "observe:32",
            "page:64",
            "observe:6",
            "page:70"
        ]
    );
    assert_eq!(
        flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
    );
    assert_eq!(
        provenance
            .observed
            .borrow()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        EXPECTED_BINDINGS,
        "binding identities and multiplicity"
    );
}

#[test]
fn observes_empty_bindings_once_to_preserve_basis_validation() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 0,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
        observed: RefCell::new(Vec::new()),
    };
    let node = build_knowledge();
    // When
    let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
    // Then
    assert_eq!(*events.borrow(), ["resolve", "page:0", "observe:0"]);
    assert_eq!(flags, BTreeSet::from([SearchFlag::SourceChanged]));
}

#[test]
fn propagates_late_provenance_failure_without_partial_flags_or_further_pages() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: Some("s032.rs"),
        observed: RefCell::new(Vec::new()),
    };
    let node = build_knowledge();
    // When
    let error = observe_bindings(&mut read, &node, &provenance).unwrap_err();
    // Then
    assert_eq!(
        error.get_kind(),
        crate::service::CommandErrorKind::InternalError
    );
    assert_eq!(
        *events.borrow(),
        ["resolve", "page:0", "observe:32", "page:32", "observe:32"]
    );
}

#[test]
fn reports_late_binding_page_failure_after_completed_observations() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: Some(64),
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
        observed: RefCell::new(Vec::new()),
    };
    // When
    let error = observe_bindings(&mut read, &build_knowledge(), &provenance).unwrap_err();
    // Then
    assert_eq!(
        error.get_kind(),
        crate::service::CommandErrorKind::InternalError
    );
    assert_eq!(
        *events.borrow(),
        [
            "resolve",
            "page:0",
            "observe:32",
            "page:32",
            "observe:32",
            "page:64"
        ]
    );
}

struct BindingRead {
    events: Rc<RefCell<Vec<String>>>,
    count: usize,
    fail_at: Option<usize>,
}
struct BindingProvenance {
    events: Rc<RefCell<Vec<String>>>,
    fail_on: Option<&'static str>,
    observed: RefCell<Vec<String>>,
}
impl GraphRead for BindingRead {
    fn resolve_node_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        assert_eq!(id.as_str(), "b.md");
        self.events.borrow_mut().push("resolve".into());
        Ok(Some(GraphNodeRef {
            key: GraphNodeKey::new(1),
            element: EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
        }))
    }
    fn get_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        assert_eq!(request.limit.get(), 32);
        assert_eq!(
            request
                .edge_types
                .iter()
                .map(EdgeType::as_str)
                .collect::<Vec<_>>(),
            ["external"]
        );
        let start = request.after.map_or(0, |cursor| {
            cursor.neighbor.as_str()[1..4].parse::<usize>().unwrap() + 1
        });
        self.events.borrow_mut().push(format!("page:{start}"));
        if self.fail_at == Some(start) {
            return Err(crate::service::StorageError::new(
                crate::service::StorageErrorKind::InternalError,
                "late binding page unavailable",
            ));
        }
        Ok((start..self.count)
            .take(32)
            .map(|index| {
                let target =
                    EdgeTarget::Source(SourceUri::parse(&format!("s{index:03}.rs")).unwrap());
                let edge = Edge::new(
                    KnowledgeId::parse("b.md").unwrap(),
                    target.clone(),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap();
                GraphNeighbor {
                    node: GraphNodeRef {
                        key: GraphNodeKey::new(i64::try_from(index + 2).unwrap()),
                        element: target,
                    },
                    edge,
                }
            })
            .collect())
    }
}
impl SourceProvenance for BindingProvenance {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("unexpected revision validation")
    }
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        assert!(request.sources.len() <= 32);
        self.events
            .borrow_mut()
            .push(format!("observe:{}", request.sources.len()));
        self.observed.borrow_mut().extend(
            request
                .sources
                .iter()
                .map(|source| source.as_str().to_owned()),
        );
        let first = request.sources.first().map(SourceUri::as_str);
        if self.fail_on.is_some() && first == self.fail_on {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                "private provenance failure",
            ));
        }
        let flag = if first == Some("s032.rs") {
            SearchFlag::SourceNotFound
        } else {
            SearchFlag::SourceChanged
        };
        Ok(ProvenanceObservation {
            flags: BTreeSet::from([flag]),
            issues: BTreeSet::new(),
        })
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("unexpected revision comparison")
    }
}
fn build_knowledge() -> KnowledgeMetadata {
    KnowledgeMetadata {
        id: KnowledgeId::parse("b.md").unwrap(),
        uri: KnowledgeId::parse("b.md").unwrap(),
        kind: ElementKind::Vertex,
        hash: Sha256::digest(b"fixture"),
        created_at: 3,
        source_snapshot: None,
        lines: LineRange::new(1, 2).unwrap(),
        search_hash: Sha256::digest(b"search"),
    }
}
