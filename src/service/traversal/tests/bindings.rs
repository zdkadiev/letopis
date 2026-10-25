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

#[test]
fn observes_large_source_bindings_pagewise() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
    };
    let node = build_knowledge();
    let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
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
}

#[test]
fn observes_empty_bindings_once_to_preserve_basis_validation() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 0,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
    };
    let node = build_knowledge();
    let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
    assert_eq!(*events.borrow(), ["resolve", "page:0", "observe:0"]);
    assert_eq!(flags, BTreeSet::from([SearchFlag::SourceChanged]));
}

#[test]
fn propagates_late_provenance_failure_without_partial_flags_or_further_pages() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: None,
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: Some("s032.rs"),
    };
    let node = build_knowledge();
    let error = observe_bindings(&mut read, &node, &provenance).unwrap_err();
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
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut read = BindingRead {
        events: events.clone(),
        count: 70,
        fail_at: Some(64),
    };
    let provenance = BindingProvenance {
        events: events.clone(),
        fail_on: None,
    };
    let error = observe_bindings(&mut read, &build_knowledge(), &provenance).unwrap_err();
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
}
impl GraphRead for BindingRead {
    fn resolve_node(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        assert_eq!(id.as_str(), "b.md");
        self.events.borrow_mut().push("resolve".into());
        Ok(Some(GraphNodeRef {
            key: GraphNodeKey::new(1),
            element: EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
        }))
    }
    fn read_neighbor_page(
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
        for source in request.sources {
            let index = source.as_str()[1..4].parse::<usize>().unwrap();
            let observed = self
                .events
                .borrow()
                .iter()
                .filter(|event| event.starts_with("observe:"))
                .count();
            assert_eq!(
                index / 32 + 1,
                observed,
                "source belongs to exactly this page"
            );
        }
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
