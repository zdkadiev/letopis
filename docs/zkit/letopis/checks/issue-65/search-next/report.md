# Bounded hybrid search: research, 2026-11-07

Status: research and proposed design; no product changes or new latency measurements.
The user clarified that database retrieval itself should return a bounded number
of candidates, suggesting a default of 100. This supersedes treating complete
candidate enumeration as an unconditional requirement for the next design.
The exact scope of 100 (per channel versus combined pool) remains a proposal below.

## Current evidence

The latest whole-query median is 65.829875 seconds on a synthetic 100k corpus
with real Ollama vectors, query `case 000000`, final limit 5. The genuine Samply
profile attributes 96.7047% inclusive sampled CPU to candidate eligibility and
0.9954% to the native vector retrieval owner. These overlap other scopes and
are not wall-time shares. Sources: ../vector-index/report.md and its profile.

Storage currently returns complete FTS and vector channels. Search validates
metrics/metadata, observes each candidate's current knowledge and provenance,
filters, recomputes eligible channel ranks, fuses reciprocal ranks, then limits.
Source: src/service/search.rs, src/storage/cache.rs, src/storage/vector.rs.
Without filters, flags decorate results rather than excluding candidates.
An exact winner-first path could preserve successful static ranking but changes
live/provenance error detection outside the returned results. It does not itself
satisfy the user's clarified requirement to bound database results.

## Recommended bounded candidate proposal

Ask FTS for its best 100 knowledge hits ordered by BM25 and ID; ask vector search
for its best 100 knowledge hits ordered by cosine distance and ID. Deduplicate
the union: at most 200 knowledge candidates. Keep result limit separate from this
candidate budget. Preserve fresh observations for these candidates, apply filters,
then fuse eligible ranks within this bounded pool. This is a product contract
change: it does not guarantee the same results as complete-corpus eligible RRF.
No new ANN index is needed for bounded exact native top-K retrieval.

A vector's search_hash can map to multiple knowledge nodes. A bound on native
vector rows alone does not bound joined knowledge hits; deterministic node-level
top-K, ties, shared vectors and bounded output must be covered explicitly.
A SQL LIMIT bounds returned results, not necessarily the database's internal
work. Inspect actual FTS/vec1 plans and native cost rather than assuming O(100).

A fixed candidate budget can return fewer results after filters even when more
eligible nodes exist outside the pool. Unbounded refill would reintroduce the
original worst case. A separate bounded refill policy is an alternative, not
silently part of this proposal. Requests above the candidate budget, default
versus explicit result limit, and per-channel versus total pool need design.

## Validation proposed before claiming a speedup

Compare bounded versus current results on literal, semantic, mixed, tie/shared
vector and selective-filter scenarios; report recall/order changes, not just time.
Count returned storage rows and actual knowledge/provenance observations.
Run screening on the preserved 100k corpus with limits 5 and 100; only then run
three ordinary pairs and a separate genuine Samply profile. No timing target or
speedup is established by this research. Existing historical evidence stays intact.
