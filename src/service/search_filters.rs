use super::SearchFlag;
use crate::domain::{SourceUri, TreeOid};
use std::collections::BTreeSet;

/// Select knowledge by whether its current X-1 flags require rechecking.
/// An empty flag set does not certify truth or remote availability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecheckFilter {
    /// Impose no restriction on current flags.
    #[default]
    Any,
    /// Require at least one current freshness flag.
    Required,
    /// Require no current freshness flags.
    NotRequired,
}

/// Normalized search restrictions; fields combine with AND and list values with OR.
/// Empty lists impose no restriction. Sources and revisions must match one binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// Restriction on current knowledge and source freshness flags.
    pub needs_recheck: RecheckFilter,
    /// Exact canonical source identities, deduplicated and ordered.
    pub sources: BTreeSet<SourceUri>,
    /// Complete requested tree OIDs, deduplicated in validation order.
    pub source_revisions: BTreeSet<TreeOid>,
}

impl SearchFilters {
    /// Return whether the observed flags satisfy the recheck restriction.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Search eligibility uses this predicate in TK-22 Task 2."
        )
    )]
    pub(crate) fn matches_flags(&self, flags: &BTreeSet<SearchFlag>) -> bool {
        match self.needs_recheck {
            RecheckFilter::Any => true,
            RecheckFilter::Required => !flags.is_empty(),
            RecheckFilter::NotRequired => flags.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RecheckFilter, SearchFilters};
    use crate::service::SearchFlag;
    use std::collections::BTreeSet;

    #[test]
    fn default_filters_impose_no_restrictions() {
        // Arrange
        let filters = SearchFilters::default();
        let observations = [
            BTreeSet::new(),
            BTreeSet::from([SearchFlag::Stale]),
            BTreeSet::from([SearchFlag::SourceChanged]),
            BTreeSet::from([SearchFlag::SourceNotFound]),
        ];
        // Act / Assert
        assert_eq!(filters.needs_recheck, RecheckFilter::Any);
        assert!(filters.sources.is_empty());
        assert!(filters.source_revisions.is_empty());
        for flags in observations {
            assert!(filters.matches_flags(&flags), "{flags:?}");
        }
    }

    #[test]
    fn required_recheck_matches_each_current_flag_and_rejects_no_flags() {
        // Arrange
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::Required,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), false),
            (BTreeSet::from([SearchFlag::Stale]), true),
            (BTreeSet::from([SearchFlag::SourceChanged]), true),
            (BTreeSet::from([SearchFlag::SourceNotFound]), true),
        ];
        // Act / Assert
        for (flags, expected) in cases {
            assert_eq!(filters.matches_flags(&flags), expected, "{flags:?}");
        }
    }

    #[test]
    fn not_required_recheck_matches_no_flags_and_rejects_each_current_flag() {
        // Arrange
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::NotRequired,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), true),
            (BTreeSet::from([SearchFlag::Stale]), false),
            (BTreeSet::from([SearchFlag::SourceChanged]), false),
            (BTreeSet::from([SearchFlag::SourceNotFound]), false),
        ];
        // Act / Assert
        for (flags, expected) in cases {
            assert_eq!(filters.matches_flags(&flags), expected, "{flags:?}");
        }
    }
}
