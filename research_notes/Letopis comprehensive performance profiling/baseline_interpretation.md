# Canonical baseline interpretation

## Evidence and scope

Read-only analysis of existing observations; no workload, build or sampler executed. This worker's evidence is exclusively the canonical survey: **60/1k/10k, not 100k**. Stock revision `0046a6d`, disabled embeddings/local FTS, 352 successful observations: 319 measured, 30 conditioning, three first-use. Never pool phases, configurations, builds or stage-wrapper times. [Survey](canonical_survey.md), raw [60](evidence/canonical-survey-60-20261117-v1/samples.jsonl), [1k](evidence/canonical-survey-1000-20261117-v1/samples.jsonl), [10k](evidence/canonical-survey-10000-20261117-v1/samples.jsonl).

Ratios below independently recomputed from raw **measured-phase** medians: 10k ÷ 1k. CPU = user+system wait4; RSS = individual lifetime peak, not allocations or simultaneous process-family memory.

| Exact scenario ID | n 1k/10k | Wall ratio | CPU ratio | RSS ratio |
|---|---:|---:|---:|---:|
| `search` | 20/20 | 1.745× | 2.133× | 1.080× |
| `inspect` | 20/3 | 14.581× | 9.168× | 1.383× |
| `traverse` | 20/20 | 2.091× | 2.767× | 1.071× |
| `check` | 20/20 | 7.661× | 8.730× | 3.518× |
| `status` | 20/20 | 8.118× | 8.756× | 3.546× |
| `refresh_one` | 3/3 | 6.001× | 8.968× | 2.907× |
| `refresh_several` | 3/3 | 4.742× | 7.344× | 2.749× |
| `refresh_bulk` | 3/3 | 12.526× | 13.686× | 3.435× |
| `reindex` | 3/3 | 12.669× | 13.861× | 3.498× |

Independent [evidence audit](canonical_evidence_audit.md) confirms current survey tables and resource JSON match all 352 raw rows within displayed rounding. Current 10k median RSS: check **62.47**, status **59.40**, refresh_one **51.73 MiB**. The earlier discrepancy claim was stale; no raw correction is needed.

Provenance limits: command mappings below derive from the archived runner recipe, not per-process argv/stdin traces. Disabled configuration supports intended zero provider calls; network traffic was not independently traced. Recorded success/oracle flags are not a correctness rerun.

## Associations, not causes

Search/traverse grow much less than the tenfold corpus increase, consistent with bounded retrieval/BFS plus shared refresh overhead (H1/H3/H9/H11). This does not isolate discovery cost or establish asymptotic bounds.

Check/status have similar scaling and 10k CPU-median/wall-median ratios 0.973/0.981: substantial process CPU accompanies growth, consistent with global diagnostics/provenance H5/H6/H12. Ratios are not sampled CPU attribution.

Bulk/full wall grows ~12.5× and CPU ~13.7–13.9×; associated RSS growth and small-change amplification justify H4/global reconciliation investigation, not a proven quadratic algorithm. Archived recipes map `refresh_*` to search after edits and `reindex` to `index --full`.

Inspect conditioning **4.553→0.662 s (6.878×)** is observed variation, not proven cache/object-reuse/fsync causation. CPU also changes **1.548→0.526 s**; measured calls span **0.734–1.414 s**, n=3. First capture/reused objects, OS cache and host scheduling remain hypotheses. Adaptive reduction was triggered by first conditioning duration; do not discard it or merge it into measured medians.

## Twelve highest-information sampler targets

Use existing canonical IDs at **10k**, with matched 1k captures where budget permits. Supplement IDs come from [coverage review](coverage_review.md); these remain unmeasured. All recommendations are hypotheses awaiting samples; authorities/functions in [hot-path notes](hot_path_hypotheses.md).

1. `check`: global diagnostics versus historical/live provenance, H5/H6/H12.
2. `status`: shared path without refresh; compare attribution with check.
3. `refresh_bulk`: parser, graph reconciliation, SQL/commit, H4/H7.
4. `reindex`: full verification/publication versus bulk edit.
5. `refresh_one`: fixed changed-owner work versus global metadata, H4.
6. `inspect`: separate first-capture and reused-state profiles, H8; retain conditioning.
7. `search`: ordinary small-result startup/discovery floor, H1/H3.
8. `traverse`: BFS versus hydration/provenance, H11.
9. `IDX-01` unchanged incremental: direct index baseline without search execution.
10. `IDX-03` unchanged verify: closure audit versus ordinary retention.
11. `MEM-05` deterministic-local hybrid, fixed coverage/L100, dimensions384/768: H9/native scaling; require `VEC-01` participation oracle.
12. `DRN-01` direct backlog drain: selection/reparse/reservation/commit, H13; distinct disabled/empty/busy controls.

Samply first; `sample` only if blocked. Separate profiled/unprofiled runs and child/mock scopes. Localhost measures protocol/client work, not real model compute or internet latency. No causal stack attribution exists yet.
