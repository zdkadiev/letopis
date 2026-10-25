use super::SearchFlag;
use crate::domain::{SourceUri, TreeOid};
use std::collections::BTreeSet;

/// One knowledge element's basis and source addresses from one consistent view.
#[derive(Debug, Clone, Copy)]
pub struct ProvenanceRequest<'a> {
    /// Author-declared common source tree, never replaced with current HEAD.
    pub source_snapshot: Option<&'a TreeOid>,
    /// Sources belonging to that same element and view.
    pub sources: &'a BTreeSet<SourceUri>,
}
/// Current provenance findings without source text or persistent state.
#[derive(Debug, PartialEq, Eq)]
pub struct ProvenanceObservation {
    /// Deduplicated existing X-1 reasons for rechecking the knowledge.
    pub flags: BTreeSet<SearchFlag>,
    /// Exact current causes for owner-scoped check diagnostics.
    pub issues: BTreeSet<ProvenanceIssue>,
}
/// A current validation finding rather than an infrastructure error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProvenanceIssue {
    /// Referenced sources have no declared basis.
    MissingSnapshot,
    /// The declared root is absent or is not a valid tree for this repository.
    SnapshotUnavailable {
        /// Author-declared identity, preserved without substitution.
        snapshot: TreeOid,
    },
    /// The full local address is absent in either the current file or basis.
    SourceNotFound {
        /// The exact source address, including its inclusive range.
        source: SourceUri,
    },
}
/// Compare one source address in its knowledge's basis and an explicit revision.
#[derive(Debug, Clone, Copy)]
pub struct SourceRevisionRequest<'a> {
    /// Basis belonging to the same binding as `source`.
    pub source_snapshot: Option<&'a TreeOid>,
    /// This one address is selected identically from both trees.
    pub source: &'a SourceUri,
    /// Exact requested tree; missing or invalid roots are request errors.
    pub revision: &'a TreeOid,
}
/// Exact byte comparison, distinguishing unavailable inputs from inequality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRevisionComparison {
    /// Both selected byte sequences are available and equal.
    Equal,
    /// Both selected byte sequences are available and unequal.
    Different,
    /// A complete local path or range is absent in one tree.
    SourceNotFound,
    /// The knowledge has no usable declared basis.
    BasisUnavailable,
    /// Remote bytes are unknown because no fetch is performed.
    Unfetched,
}
