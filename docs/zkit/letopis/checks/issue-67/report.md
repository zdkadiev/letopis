# Product Git optimization (#67): corrected measurement report

After the final review's sparse-index fix, fresh matched 60-element check median
is **13.486335 s → 0.031300 s** (10 valid observations per side),
**99.768% lower, 430.87×**. The provisional 50% goal is met
on this workload. F-I1 and F-M1 have focused fixes and refreshed evidence;
**scoped independent re-review remains pending**, owned by the controller.

## Current before/after results

All after cells below refer to corrected product `b488aeb4a7faa33f58e6e9c87a4f4d638631680b`.
All four ordinary before/after series are newly matched and interleaved. Baseline
publication, known maintenance censors, contention and diagnostic observations are
reused unchanged from Task 8, explicitly historical; they are not new paired runs.
Original optimized `ba4a11d` measurements and all raw receipts remain immutable.

W is maximum observed wait4 lifetime RSS, including OS-accounted reaped descendants,
not simultaneous process-tree peak. D is one separately instrumented direct-PID
sampled RSS peak per side/scenario, excluding descendants; D before is historical,
D after is fresh. MiB = 1,048,576 bytes. Neither establishes whole-stack/NFR RAM.

| Scenario | Before median / n | After median / n | Reduction / speedup | Git subprocesses before → after | Peak memory before → after / scope | Correctness / limits |
| --- | --- | --- | --- | --- | --- | --- |
| Initialized check, 60 elements with external links | 13.486335 s / 10 | 0.031300 s / 10 | 99.768% / 430.87× | 1183 → 0 | W 12.09 → 13.22 MiB; D 11.92 → 12.83 MiB | 10/10 valid each; 2 warmups each excluded |
| Initialized inspect, 1k | 4.015773 s / 10 | 0.384517 s / 10 | 90.425% / 10.44× | 375 → 0 | W 12.80 → 14.67 MiB; D 12.33 → 13.88 MiB | 10/10 valid each; 2 warmups each excluded |
| Bounded traverse, 1k | 1.803754 s / 10 | 0.029186 s / 10 | 98.382% / 61.80× | 180 → 0 | W 13.03 → 14.19 MiB; D 12.75 → 13.98 MiB | 10/10 valid each; 2 warmups each excluded |
| FTS search, 1k (control) | 0.672846 s / 10 | 0.023278 s / 10 | 96.540% / 28.90× | 66 → 0 | W 12.89 → 14.20 MiB; D 12.88 → 14.05 MiB | 10/10 valid each; 2 warmups each excluded |
| First publication and full audit, 1k | Historical publication 195.466365 s / 1; audit 0/1 success, 1 timeout | Publication 0.373097 s / 1; audit 0.197994 s / 3 | Publication 99.809% / 523.90×; audit not assessed | Not separately counted | Publication W historical 20.00 → 20.91 MiB; audit W after 17.94 MiB | Before reused unchanged; after fresh; n=1 publication; 600/120 s caps |
| Reindex with concurrent reader, 60 | Historical reader 12.098960 s / 3 | Reader 0.029425 s / 3 | 99.757% / 411.17× | Not separately counted | Reader W historical 12.55 → 13.89 MiB | Fresh after 3/3 lock acknowledgement and overlap; historical before reused |

Counters are separate instrumentation: the four unchanged baseline counters and
four fresh corrected-pin counters. No counter is invented for publication/audit or
contention. Current Linux source tracing independently recorded 41 foreground/
direct-drain invocations, including two sparse inspect cases. Eligible foreground
to detached drain is covered by existing lifecycle tests/source audit, not those
41 trace intervals alone.

| Scenario | Before min–max, seconds | After min–max, seconds | Counts |
| --- | --- | --- | --- |
| check | 12.565744–18.642162 | 0.027357–0.040015 | 10/10; 0 censored/invalid |
| inspect | 3.769416–4.437270 | 0.346629–0.678846 | 10/10; 0 censored/invalid |
| traverse | 1.697170–2.083727 | 0.026979–0.042940 | 10/10; 0 censored/invalid |
| search | 0.605890–0.704774 | 0.019482–0.026263 | 10/10; 0 censored/invalid |

Each ordinary series excludes two warmups per side; projection revisions stay
constant within each series. No p95/p99 is estimated from n=10. Publication has
n=1, not a stable distribution. Historical before versus fresh after publication
and contention ratios are descriptive and are not a contemporaneous paired estimate.

## Audit and maintenance budgets

The fresh after-only stage does not repeat any known baseline timeout. Each call
has a 120-second cap; first publication has a 600-second cap; each refresh stage
has a 1,200-second watchdog including setup. No scale exceeds 1k and no paid/model
or real-provider session was used.

| Scenario, 1k | Before, historical Task 8 | After fresh median / n; min–max |
| --- | --- | --- |
| check | 0/1 successful; timeout at 120 s; 2 skipped | 0.197994 s / 3; 0.181144–0.203690 s |
| reindex | 0/1 successful; timeout at 120 s; 2 skipped | 0.345995 s / 3; 0.345418–0.356993 s |
| status | 0/1 successful; timeout at 120 s; 2 skipped | 0.192248 s / 3; 0.189780–0.206465 s |
| refresh_bulk | 0/1 successful; timeout at 120 s; 2 skipped | 0.418491 s / 3; 0.372432–0.419393 s |

The historical initialization stage remains incomplete: 18 attempts, 14 successes,
four timeouts and exit 1. Those censors never become successful latencies, and no
audit/maintenance speedup is inferred. Its earlier read-stage handoff note was
incorrectly copied to four internal command entries; the fresh correction archive
uses their own command/progress/sample receipts (18/14/4, 8/8, 8/8, 20/20).

The fresh stage ledger is complete (all exit 0); warmups are counted here but
excluded from ordinary medians. Profile controls/counters and RSS have separate
receipts and are not pooled into ordinary samples.

| Fresh stage | Top-level attempts | Valid | Timeouts |
| --- | --- | --- | --- |
| check60 | 24 | 24 | 0 |
| inspect/traverse/search1k | 72 | 72 | 0 |
| after publication/maintenance/contention | 16 | 16 | 0 |
| after profile recordings | 12 | 12 | 0 |

## Profiles, counters and memory

Twelve fresh after recordings (three per ordinary scenario), each paired with an
unprofiled overhead control, and four Git counters use existing profile helpers.
Four fresh after direct-RSS measurements reuse the validated Task 8 libproc sampler.
Original baseline profiles/counters/RSS remain valid historical inputs; all former
optimized diagnostics at `ba4a11d` are historical and are not attributed to the fix.
Profile wall time includes instrumentation overhead; inclusive sampled CPU is not
a subtractable fraction of ordinary latency. Short optimized calls may have few
samples, so no precise residual bottleneck percentage or optimization claim follows.
Raw stacks, profile receipts, plain controls and sampled RSS identities are indexed.

Task 2's unchanged object/delta allocation proof still applies to the unchanged
reader: 64 MiB delta largest allocation 134,221,096 bytes, retained capacity 0,
largest child RSS 146,800,640 bytes; 1 MiB delta retained 2,097,233 bytes. Hash chunks
are 64 KiB; reusable allocation cap is 4 MiB, not an input cap. Sparse expansion
keeps pending path/OID/mode metadata and one verified tree body at a time; it adds
no payload cache. Ordinary non-sparse calls borrow the existing index and do not
read expansion objects or clone the index.

## Source, fixtures and verification

Baseline `881cc6417073110e852fbebf2b9bce5ed1ea3d29` has 156 archived inputs equivalent
to product base `32ca0af282a1321d0137b4aba115c6fddc086259`; corrected optimized
`b488aeb4a7faa33f58e6e9c87a4f4d638631680b` has 171 inputs.
Binary SHA-256:

- Before: `a132acb0a19c24440e8e97b881657c7b7adcc5c17052469f5dbc9d1e1b336f62`
- After: `950f522946a4611885f6ef8fbb26e94b336c45c9a9fa7f1b328e41b6ba9050d3`

Both use Rust 1.95.0 and `cargo build --release --locked --offline --message-format=json`
with `CARGO_PROFILE_RELEASE_DEBUG=1`. Original source archives, binaries and build
receipts are immutable; current source equality and both pins were verified.

The narrow archived refresh driver calls the existing `run_timing_series`,
`prepare_scenario`, validation, measurement, profile and contention helpers. It
reuses Task 8 initialized fixture repositories only as read-only copy sources,
verifies corpus SHA-256/config/non-sparse index/projection revision, archives their
complete file hashes, and checks those originals again after each stage. Scenario
preparation refreshes copied inode identities outside measurement. Seed 65,
2,000-byte fragments, 30 elements/file and disabled embeddings are unchanged.
OS page cache is uncontrolled; host load and exact driver/harness identities are
recorded. The 195-second baseline first publication was not redundantly rerun.

`make verify` passed on the corrected tree: format, all-target check, Clippy,
debug tests, strict Rustdoc and release tests. Focused Linux aarch64 Git 2.50.1
regressions passed 22 tests plus 5 no-helper tests/41 traced invocations. Two failed
Linux setup attempts remain visible: old trace image lacked a current config
fixture, then exposed its Git 2.39 strict-oracle mismatch. The successful fresh
image uses the pinned 2.50.1 oracle and current source overlay.

The real SHA-1/SHA-256 sparse-index regression verifies exact candidate parity and
unrequested inspect completeness, staged membership distinct from HEAD, deleted
and ignored behavior, nested missing ignore files, raw replacement immunity,
missing/corrupt object errors, late alternates and preserved user state. Omitting
expansion compiles and fails the literal membership assertion; source was restored.
No changes cross the Git module/service boundary. Storage/native and large-object
campaigns were not rerun: their code/dependencies are unchanged. Historical harnesses,
x86_64, NFS, physical power loss, whole-stack RAM and real-provider performance are
not claimed. The direct-drain wrong-root suspicion is disposed of by the actual
root-bound executor call path in [review.md](review.md); no dynamic subdirectory
contention test was performed. M3 retains its narrow initial-list mutation scope.

The [canonical evidence index](evidence-index.json) links fresh and historical
receipts separately. [Compatibility](compatibility.md) records exact platform
scope. Issue 65 worktree files and staged R-10 remain untouched; no push, merge,
GitHub write or issue closure occurred. Only the controller can close scoped review.
