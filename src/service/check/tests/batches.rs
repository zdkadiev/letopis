use super::{Probes, Verifier, build_node, open_storage, publish_nodes};
use crate::{
    domain::TreeOid,
    service::{
        DocumentResult, GraphDiagnosticsService, ProvenanceObservation, ProvenanceRequest,
        SnapshotError, SnapshotState, SourceProvenance, SourceRevisionComparison,
        SourceRevisionRequest, Storage, check_problems::collect_check_problems,
    },
};
use std::{cell::RefCell, collections::BTreeSet};

struct Batches {
    verifier: Verifier,
    sizes: RefCell<Vec<usize>>,
}

#[test]
fn check_batches_selected_owners_without_losing_diagnostic_order() {
    // Given: 257 owners with unavailable bases in the already pinned metadata.
    let directory = tempfile::tempdir().unwrap();
    let mut storage = open_storage(&directory);
    let nodes: Vec<_> = (0..257)
        .map(|n| build_node(&format!("f{n:03}.md"), "a"))
        .collect();
    publish_nodes(&mut storage, &nodes);
    let provenance = Batches {
        verifier: Verifier {
            probes: Probes::default(),
            result: SnapshotState::Missing,
            fail: false,
        },
        sizes: RefCell::new(Vec::new()),
    };
    let mut read = storage.begin_read_tx().unwrap();
    // When: common check/status problem collection observes these owners.
    let problems =
        collect_check_problems(&mut read, &[], &GraphDiagnosticsService::new(), &provenance)
            .unwrap();
    // Then: bounded batches preserve all owner mappings and sorted diagnostics.
    assert_eq!(*provenance.sizes.borrow(), [256, 1]);
    assert_eq!(problems.len(), 257);
    assert_eq!(problems[0].details["element_id"], "f000.md");
    assert_eq!(problems[256].details["element_id"], "f256.md");
    assert_eq!(provenance.verifier.probes.borrow().len(), 257);
}

impl SourceProvenance for Batches {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("check has no requested revisions")
    }
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        self.verifier.observe_sources(request)
    }
    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        self.sizes.borrow_mut().push(requests.len());
        requests
            .iter()
            .map(|request| self.observe_sources(*request))
            .collect()
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("check does not compare revisions")
    }
}
