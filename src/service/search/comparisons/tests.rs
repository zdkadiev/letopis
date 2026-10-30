use super::RevisionComparisons;
use crate::{
    domain::{SourceUri, TreeOid},
    service::{
        DocumentResult, ProvenanceObservation, ProvenanceRequest, SnapshotError, SnapshotErrorKind,
        SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
};

type ComparisonKey = (Option<TreeOid>, SourceUri, TreeOid);
#[derive(Default)]
struct Recorded {
    calls: RefCell<Vec<ComparisonKey>>,
    failure: Cell<bool>,
    result: Cell<Option<SourceRevisionComparison>>,
}

#[test]
fn exact_basis_address_range_and_revision_are_distinct() {
    // Given: requests differ in exactly one key component, including an absent basis.
    let provenance = Recorded::default();
    let mut comparisons = RevisionComparisons::new(&provenance);
    let first = TreeOid::parse(&"a".repeat(40)).unwrap();
    let second = TreeOid::parse(&"b".repeat(40)).unwrap();
    let whole = SourceUri::parse("a").unwrap();
    let range = SourceUri::parse("a#L1-L1").unwrap();
    let other = SourceUri::parse("b").unwrap();
    let requests = [
        (None, &whole, &first),
        (Some(&first), &whole, &first),
        (Some(&second), &whole, &first),
        (Some(&first), &range, &first),
        (Some(&first), &other, &first),
        (Some(&first), &whole, &second),
    ];
    // When: each request is repeated, without changing their exact identities.
    for (basis, source, revision) in requests {
        let request = SourceRevisionRequest {
            source_snapshot: basis,
            source,
            revision,
        };
        let left = comparisons.compare_revision(request).unwrap();
        let right = comparisons.compare_revision(request).unwrap();
        assert_eq!(left, right);
    }
    // Then: each exact key is demanded once, in its first-use order.
    assert_eq!(provenance.calls.borrow().len(), 6);
    assert_eq!(
        provenance.calls.borrow()[0],
        (None, whole.clone(), first.clone())
    );
    assert_eq!(provenance.calls.borrow()[5], (Some(first), whole, second));
}

#[test]
fn revision_failures_are_retried_and_overflow_recomputes() {
    // Given: one failing comparison is retried after repair.
    let provenance = Recorded::default();
    let mut comparisons = RevisionComparisons::new(&provenance);
    let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
    let source = SourceUri::parse("a").unwrap();
    let request = SourceRevisionRequest {
        source_snapshot: Some(&tree),
        source: &source,
        revision: &tree,
    };
    provenance.failure.set(true);
    assert_eq!(
        comparisons
            .compare_revision(request)
            .unwrap_err()
            .get_kind(),
        SnapshotErrorKind::InternalError
    );
    provenance.failure.set(false);
    // When: the repaired result and 4096 additional distinct addresses are demanded.
    assert_eq!(
        comparisons.compare_revision(request).unwrap(),
        SourceRevisionComparison::Equal
    );
    for n in 0..4096 {
        let source = SourceUri::parse(&format!("source{n}")).unwrap();
        comparisons
            .compare_revision(SourceRevisionRequest {
                source: &source,
                ..request
            })
            .unwrap();
    }
    let overflow = SourceUri::parse("source4095").unwrap();
    comparisons.compare_revision(request).unwrap();
    comparisons
        .compare_revision(SourceRevisionRequest {
            source: &overflow,
            ..request
        })
        .unwrap();
    // Then: the failure was retried, a cached success reused and overflow recomputed.
    assert_eq!(provenance.calls.borrow().len(), 4099);
}

#[test]
fn successful_comparison_variants_are_preserved() {
    for expected in [
        SourceRevisionComparison::Different,
        SourceRevisionComparison::SourceNotFound,
        SourceRevisionComparison::Unfetched,
    ] {
        // Given: a demanded typed result, independent of the later collaborator value.
        let provenance = Recorded::default();
        provenance.result.set(Some(expected));
        let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
        let source = SourceUri::parse("a").unwrap();
        let request = SourceRevisionRequest {
            source_snapshot: Some(&tree),
            source: &source,
            revision: &tree,
        };
        let mut comparisons = RevisionComparisons::new(&provenance);
        // When: the successful exact comparison is requested again.
        assert_eq!(comparisons.compare_revision(request).unwrap(), expected);
        provenance.result.set(Some(SourceRevisionComparison::Equal));
        let result = comparisons.compare_revision(request).unwrap();
        // Then: the original typed result is returned without more historical work.
        assert_eq!(result, expected);
        assert_eq!(provenance.calls.borrow().len(), 1);
    }
}

#[test]
fn unavailable_basis_is_rechecked_when_the_same_key_becomes_available() {
    // Given: a declared basis is unavailable on the first demanded comparison.
    let provenance = Recorded::default();
    provenance
        .result
        .set(Some(SourceRevisionComparison::BasisUnavailable));
    let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
    let source = SourceUri::parse("a").unwrap();
    let request = SourceRevisionRequest {
        source_snapshot: Some(&tree),
        source: &source,
        revision: &tree,
    };
    let mut comparisons = RevisionComparisons::new(&provenance);
    assert_eq!(
        comparisons.compare_revision(request).unwrap(),
        SourceRevisionComparison::BasisUnavailable
    );
    // When: the same basis appears before its next demand.
    provenance.result.set(Some(SourceRevisionComparison::Equal));
    let result = comparisons.compare_revision(request).unwrap();
    // Then: the negative classification cannot hide the now available bytes.
    assert_eq!(result, SourceRevisionComparison::Equal);
    assert_eq!(provenance.calls.borrow().len(), 2);
}

impl SourceProvenance for Recorded {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("caller validates roots before eligibility")
    }
    fn observe_sources(&self, _: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation> {
        panic!("comparisons never observe live bytes")
    }
    fn compare_revision(
        &self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        self.calls.borrow_mut().push((
            request.source_snapshot.cloned(),
            request.source.clone(),
            request.revision.clone(),
        ));
        if self.failure.get() {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "comparison failure",
            ));
        }
        Ok(self.result.get().unwrap_or(SourceRevisionComparison::Equal))
    }
}
