use super::{fail_invalid_search_data, map_search_error};
use crate::{
    domain::KnowledgeId,
    service::{CommandResult, TextMatch, VectorMatch},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};

/// Validate complete metrics, rank eligible channels, and fuse before the final limit.
pub(super) fn rank_candidates(
    fts: &[TextMatch],
    vectors: &[VectorMatch],
    eligible: &BTreeSet<KnowledgeId>,
    limit: NonZeroUsize,
) -> CommandResult<Vec<KnowledgeId>> {
    validate_metrics(fts, vectors)?;
    let text_ranks = rank_channel(
        fts.iter().map(|hit| (&hit.id, hit.bm25)).collect(),
        eligible,
    )?;
    let vector_ranks = rank_channel(
        vectors.iter().map(|hit| (&hit.id, hit.distance)).collect(),
        eligible,
    )?;
    let mut scores = BTreeMap::new();
    for (id, weight) in text_ranks.into_iter().chain(vector_ranks) {
        let score = scores.entry(id.clone()).or_insert(0.0);
        *score += weight;
        if !score.is_finite() {
            return Err(fail_invalid_search_data());
        }
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|(left_id, left), (right_id, right)| {
        right.total_cmp(left).then_with(|| left_id.cmp(right_id))
    });
    Ok(ranked
        .into_iter()
        .take(limit.get())
        .map(|(id, _)| id)
        .collect())
}

/// Reject corrupt native metrics before eligibility can discard their candidates.
pub(super) fn validate_metrics(fts: &[TextMatch], vectors: &[VectorMatch]) -> CommandResult<()> {
    let metrics = fts
        .iter()
        .map(|hit| hit.bm25)
        .chain(vectors.iter().map(|hit| hit.distance));
    if metrics.into_iter().any(|metric| !metric.is_finite()) {
        return Err(fail_invalid_search_data());
    }
    Ok(())
}

fn rank_channel<'a>(
    mut channel: Vec<(&'a KnowledgeId, f64)>,
    eligible: &BTreeSet<KnowledgeId>,
) -> CommandResult<Vec<(&'a KnowledgeId, f64)>> {
    channel.retain(|(id, _)| eligible.contains(*id));
    channel.sort_by(|(left_id, left), (right_id, right)| {
        // Signed zeros are equal metrics and therefore use the same identity tie-break.
        if left == right {
            left_id.cmp(right_id)
        } else {
            left.total_cmp(right)
        }
    });
    let mut seen = BTreeSet::new();
    channel.retain(|(id, _)| seen.insert(*id));
    for (index, (_, weight)) in channel.iter_mut().enumerate() {
        let rank = index
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .ok_or_else(fail_invalid_search_data)?;
        *weight = calculate_rank_weight(rank)?;
    }
    Ok(channel)
}

/// Restrict ranks to an exactly representable integer before computing their reciprocal.
pub(super) fn calculate_rank_weight(rank: NonZeroUsize) -> CommandResult<f64> {
    let rank = u32::try_from(rank.get()).map_err(map_search_error)?;
    Ok(1.0 / f64::from(rank))
}
