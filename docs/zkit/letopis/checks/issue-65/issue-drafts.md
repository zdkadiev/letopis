# Предложения оптимизаций после #65

2026-10-30. Локальные drafts для отдельного согласования/публикации.
Это не реализация и не обещанные speedups; proposed criteria не являются новыми
NFR проекта. [Диагностический отчёт](report.md) содержит baseline и ограничения.

## 1. Batch source verification within a single CLI invocation

Problem: repeated source-basis checks and per-range historical/current reads
cause 1183 Git subprocesses and median check 11.918 s on the 60-node fixture.
Removing only external declarations yields 283 calls / 2.950 s; this is causal
diagnosis, not an acceptable product change. A fake vector query limited to one
output also spends 12.345 s on eligibility/provenance at 60 nodes.

Owners: `src/service/provenance.rs`, `document_refresher/basis.rs`,
`src/git/objects.rs`, with application-owned interfaces preserved.

Proposed scope: group repeated immutable snapshot/object verification and source
byte/range reads within one command. Reuse existing batch-verification capabilities
where they satisfy the actual contract. Keep memory bounded and preserve current
source observations; do not persist an unvalidated cross-command cache or discard
provenance checks. Preserve CLI/service/domain boundaries and synchronous I/O.

Proposed measurable criterion: on the same release settings/corpus/requests,
reduce native check median and instrumented Git calls by at least 50% relative
to a freshly reproduced baseline (at least 10 successful samples), with equal
errors/flags and no material RSS regression. Treat this target as a hypothesis
until implemented and measured. Attempt the 1k audit again with explicit limits.

Regression focus: missing/corrupt/noncanonical objects, SHA-1/SHA-256, source
range unavailable, identical stat with changed bytes, edits during capture,
prospective publication roots/revision races, invalid last-valid documents,
source removals and fresh flags on the next CLI call. Include reindex/reader
overlap: diagnostic 60-node reader grows from 0.685 s to 12.704 s while writer
holds index.lock. Correctness/freshness cannot be weakened to reduce that wait.

Evidence: `.artifacts/issue-65/source-control-60-01/`, `window-100k-01/`,
`contention-60-01/`. Paid/model runs remain outside this proposal's baseline.

## 2. Batch Git verification in inspect snapshot capture

Problem: inspecting one fixed knowledge ID still captures all marked files;
`create_tree_from_blobs` verifies entries with separate Git invocations, including
intermediate/final trees. At 60 nodes, packing 2 → 6 Markdown files changes
inspect median 0.760 → 1.096 s and Git calls 74 → 106. The addressed fragment
bytes/source tree/totals are fixed; derived path IDs/includes boundaries differ.
At 1k: matched unprofiled median 4.183 s, 378 Git calls in a separate counter run.

Owners: `src/service/inspection.rs`, `src/git/objects.rs`; root service wiring
and snapshot API remain intact. Do not change full snapshot membership,
unknown-ID batch rejection or live-vs-captured consistency without separate design.

Proposed scope: replace repeated one-object checks with bounded streamed batch
validation and avoid redundant verification when the same invocation already has
equivalent verified bytes. Preserve type/hash/closure checks and all-request
preflight before writes, including malformed-tree structural checks.

Proposed measurable criterion: at least 50% fewer instrumented Git calls for
1k inspect, a lower unprofiled median on at least 10 matched successful samples,
and identical captured bytes, locations, errors and retention guarantees.
No speedup is claimed until measured; CPU profile overhead remains separate.

Regression focus: mixed local/remote sources, deleted/modified marked sources,
multiple IDs sharing a file, partial ranges, all-request not_found, corruption,
injected writes between captures, SIGINT/EOF, bounded temporary I/O and cleanup,
SHA variants, no missing snapshot references after the process exits.

Evidence: `.artifacts/issue-65/packing-control-60-01/`, `profile-1k-01/`.
Formal real-hybrid/NFR acceptance belongs to #35, not this optimization draft.
