# Product Git optimization (#67): measurement report

The fresh matched 60-element check median fell from **12.091148 s to 0.029212 s**
(10 valid observations per side): **99.758% lower, 413.92×**. The provisional 50%
check goal is met on this workload. All four ordinary paired series passed their
independent result oracles. Final whole-branch independent review is **pending**
and belongs to the controller; this measurement report does not certify it.

## Before/after results

W is the **maximum observed wait4 lifetime RSS** in the listed series: the direct
child plus OS-accounted reaped descendants, not a simultaneous process-tree peak.
D is a separate instrumented **direct-PID observed sampled RSS peak**, one call
per side/scenario, excluding descendants. Values use MiB = 1,048,576 bytes.
Neither scope measures whole-stack concurrent peak or a formal RAM/NFR gate.

| Scenario | Before median / n | After median / n | Reduction / speedup | Git subprocesses before → after | Peak memory before → after / scope | Correctness / limits |
| --- | --- | --- | --- | --- | --- | --- |
| Initialized check, 60 elements with external links | 12.091148 s / 10 | 0.029212 s / 10 | 99.758% / 413.92× | 1183 → 0 | W 12.00 → 13.22 MiB; D 11.92 → 12.98 MiB | 10/10 valid each; 2 warmups each excluded |
| Initialized inspect, 1k | 3.801722 s / 10 | 0.363249 s / 10 | 90.445% / 10.47× | 375 → 0 | W 12.86 → 14.73 MiB; D 12.33 → 14.52 MiB | 10/10 valid each; 2 warmups each excluded |
| Bounded traverse, 1k | 1.774182 s / 10 | 0.029009 s / 10 | 98.365% / 61.16× | 180 → 0 | W 13.08 → 14.28 MiB; D 12.75 → 14.05 MiB | 10/10 valid each; 2 warmups each excluded |
| FTS search, 1k (control) | 0.691989 s / 10 | 0.026381 s / 10 | 96.188% / 26.23× | 66 → 0 | W 12.84 → 14.12 MiB; D 12.88 → 14.00 MiB | 10/10 valid each; 2 warmups each excluded |
| First publication and full audit, 1k | Publication 195.466365 s / 1; audit 0/1 success, 1 timeout | Publication 0.360452 s / 1; audit 0.202988 s / 3 | Publication 99.816% / 542.28× (n=1); audit not assessed | Not separately counted | Publication W 20.00 → 21.28 MiB; audit W after 17.98 MiB; before censored | 600 s publication / 120 s audit; no audit ratio from censored baseline |
| Reindex with concurrent reader, 60 | Reader 12.098960 s / 3 | Reader 0.029398 s / 3 | 99.757% / 411.55× | Not separately counted | Reader W 12.55 → 13.72 MiB; writer scope below | All 3/3 schedules per side have kernel acknowledgement and positive lifetime overlap; exact lock wait not assessed |

Counters apply to their separate calls at the listed check60/read1k scales;
publication/full-audit1k and concurrent schedules were not separately wrapper
counted. Their missing counter numbers are not invented zeros. Optimized zero
also has Task 7's independent process tracing and source/feature/config audit,
including a negative control which catches an absolute Git helper despite PATH.
Those 39 traced invocations exclude eligible foreground-to-detached work; separate
lifecycle tests/source audit cover that boundary. See [review.md](review.md).

Ordinary successful ranges (seconds); no p95/p99 is estimated from n=10:

| Scenario / elements | Before min–max | After min–max | Counts |
| --- | --- | --- | --- |
| check / 60 | 11.767785–13.931716 | 0.027557–0.036221 | 10/10; 0 censored/invalid |
| inspect / 1000 | 3.678431–4.083259 | 0.331263–0.416002 | 10/10; 0 censored/invalid |
| traverse / 1000 | 1.708341–2.069208 | 0.024931–0.045934 | 10/10; 0 censored/invalid |
| search / 1000 | 0.611486–0.871661 | 0.021751–0.037485 | 10/10; 0 censored/invalid |

Publication is a single observation per side: 195.466365 s → 0.360452 s, not a
stable distribution. Other stages' separate initializations remain in their raw
receipts and ledger below; none is silently pooled into this table. The original
Task 1 ordinary baseline (13.612073833 s, n=10) and all issue 65 timings are
historical context, not final before values.

## Audit/maintenance budgets and complete attempt ledger

Every ordinary/probe call has a 120 s budget; first publication has an explicit
600 s exception. Each stage has a 1,200 s watchdog including fixture setup. The
single initialization stage took 691.525 s for all four requested scenarios.
There was no scale expansion beyond 1k and no retry after an unresolved timeout.

| Initialization scenario / 1k | Before | After median / n; min–max | Comparison |
| --- | --- | --- | --- |
| check | 0/1 successful; timeout 120.006362 s; 2 skipped | 0.202988 s / 3; 0.201445–0.206307 s | no comparable baseline median |
| reindex | 0/1 successful; timeout 120.006802 s; 2 skipped | 0.396525 s / 3; 0.373384–0.420537 s | no comparable baseline median |
| status | 0/1 successful; timeout 120.007140 s; 2 skipped | 0.183395 s / 3; 0.177379–0.196626 s | no comparable baseline median |
| refresh_bulk | 0/1 successful; timeout 120.007142 s; 2 skipped | 0.393945 s / 3; 0.392844–0.430523 s | no comparable baseline median |

Baseline censoring makes the initialization stage **incomplete**, rather than a
passing performance comparison. It does not hide the four failed attempts or
turn their 120 s limits into successful latency. Each feasible after-side has
three fresh equivalent attempts. The narrow initialization scheduler correction
retains the first timeout and records `subsequent_attempts_skipped:2`; it does not
suppress successful transports whose outputs fail correctness validation.

The following counts include each stage's first-publication calls, excluded
conditioning calls where applicable, and measured top-level records. Composite
profile ordinary/counter calls and contention baseline/writer calls additionally
remain in their individual receipts; they are not extra ordinary samples.

| Stage | Summary outcome | Top-level attempts | Valid successes | Timeouts | Interrupted |
| --- | --- | --- | --- | --- | --- |
| check60 | complete | 26 | 26 | 0 | 0 |
| reads1k | complete | 74 | 74 | 0 | 0 |
| initialization1k | incomplete | 18 | 14 | 4 | 0 |
| contention60 | complete | 8 | 8 | 0 | 0 |
| profiles60 | complete | 8 | 8 | 0 | 0 |
| profiles1k | complete | 20 | 20 | 0 | 0 |

No stage watchdog expired. Raw validation outcomes and any failure receipt were
read independently of command exit status. Initialization exit 1 is retained.
The first sequencing driver was replaced while read1k was active: only the exact
owned driver PID 94907 received SIGTERM (exit 143); stage PID 11424 kept its own
watchdog/session cleanup and was observed reparented to launchd. Its durable
`complete` summary, all 74 successful calls and later PID absence are preserved.
The replacement cannot waitpid a non-child; launchd reaping is inferred, not
claimed from an unavailable wait status. No read sample was interrupted. Original
driver source and handoff/start identities are archived.

## Contention

| Schedule | Kernel lock acknowledged | Lifetime overlap s | Baseline reader s | Overlap reader s | Writer s | Writer W MiB |
| --- | --- | --- | --- | --- | --- | --- |
| probe-search-0-after | True | 0.009942042 | 0.021614 | 0.029398 | 0.043495 | 13.83 |
| probe-search-0-before | True | 11.549236292 | 0.669191 | 11.839908 | 11.770149 | 13.06 |
| probe-search-1-after | True | 0.006499375 | 0.021648 | 0.026652 | 0.041851 | 13.92 |
| probe-search-1-before | True | 11.872643250 | 0.706193 | 12.169828 | 12.228087 | 12.81 |
| probe-search-2-after | True | 0.010664209 | 0.021788 | 0.033812 | 0.048360 | 14.03 |
| probe-search-2-before | True | 11.752641000 | 0.646946 | 12.098960 | 11.983573 | 13.00 |

The observer acknowledges the actual isolated writer's kernel index lock and
records positive CLI lifetime overlap for all six schedules. This is a reader
latency comparison under a demonstrated reindex schedule, not an exact flock or
SQLite wait measurement. No product sleep or artificial slowing was added.
Writer and reader lifetime maxima cannot be summed into concurrent peak RAM.

## Separate counters, profiles and remaining work

Samply 0.13.1 ran save-only at 1,000 Hz with presymbolicated sidecars, without a
browser, server or upload. Each side/scenario has three alternating plain/profiled
pairs; check uses 60, inspect/traverse/search use 1k. The three plain calls in each
profile series remain overhead controls, outside ordinary n=10 distributions.

| Profile scenario / side | Attempted / recorded | Plain median s | Profiled median s | Median paired wrapper ratio | Usable stack range |
| --- | --- | --- | --- | --- | --- |
| check / after | 3 / 3 | 0.034136 | 1.379691 | 40.51× | 26–37 |
| check / before | 3 / 3 | 12.067274 | 13.421999 | 1.12× | 8129–8154 |
| inspect / after | 3 / 3 | 0.527837 | 1.395274 | 2.64× | 327–435 |
| inspect / before | 3 / 3 | 4.274610 | 5.385808 | 1.22× | 2598–2651 |
| search / after | 3 / 3 | 0.027018 | 1.352085 | 49.34× | 22–29 |
| search / before | 3 / 3 | 0.654350 | 1.354543 | 2.07× | 459–482 |
| traverse / after | 3 / 3 | 0.030291 | 1.354522 | 44.52× | 33–40 |
| traverse / before | 3 / 3 | 1.831750 | 2.345436 | 1.28× | 1253–1267 |

Short optimized check recordings contain few samples; sampler startup/save costs
dominate their end-to-end profile duration. Inclusive CPU costs overlap and omit
blocked-time accounting. They are not additive and cannot be transferred to wall
time or subtracted from ordinary CLI latency. Profile and counter durations here
are explicitly instrumented observations.

| Counter scenario / side | Git calls | Instrumented seconds | Outcome |
| --- | --- | --- | --- |
| check / before | 1183 | 45.203420 | success, valid=True |
| check / after | 0 | 0.030523 | success, valid=True |
| inspect / before | 375 | 15.157850 | success, valid=True |
| inspect / after | 0 | 0.377385 | success, valid=True |
| traverse / before | 180 | 7.029317 | success, valid=True |
| traverse / after | 0 | 0.028499 | success, valid=True |
| search / before | 66 | 2.872434 | success, valid=True |
| search / after | 0 | 0.024864 | success, valid=True |

The final profile interpretation is recorded in `profile-findings.md` in the local
control directory with representative stack identities and unit/scope limits.
Current test-level work counters (not release instrumentation): two identical
bindings in one batch cause one historical root validation/read/hash and two live
captures; two batches plus one singleton cause three root checks/reads and five
live captures. The 4,096-entry aggregate root/hash cap recomputes on exhaustion:
4,097 historical reads and 4,098 live captures in the recorded overflow fixture.
257-owner check/status/preflight requests use batches `[256,1]`. Search/traversal
preserve singleton/live observation order; no cross-binding history reuse is
claimed for those consumers. Task 7 assembly counts one read per unique supplied
OID and retains independent final closure validation. Exact release object/hash/
source-observation counts and exact SQL/lock wait times were not collected.

Task 2's separately scoped object/delta allocation proof remains the evidence for
payload release: 64 MiB delta largest allocation 134,221,096 bytes, retained decode
capacity 0, largest child RSS 146,800,640 bytes; 1 MiB delta retained 2,097,233 bytes.
Hash chunks are 64 KiB, reuse capacity is capped at 4 MiB, object/delta payload
caches are disabled. Working memory can scale with the current object/delta;
neither these facts nor sampled RSS establish constant audit memory.

## Provenance, correctness and reproducibility

Baseline revision `881cc6417073110e852fbebf2b9bce5ed1ea3d29` has 156 archived product
inputs equivalent to product base `32ca0af282a1321d0137b4aba115c6fddc086259`.
Optimized revision `ba4a11db26705999018187e8301a51e9942fe1ba` has 171 inputs. Binary
SHA-256 before/after:

- `a132acb0a19c24440e8e97b881657c7b7adcc5c17052469f5dbc9d1e1b336f62`
- `18288faf715310d6ceccd03bedd35adc21e1ab1654c4765c30eff1f3a85a2870`

Both original receipts and read-only sources/artifacts were reverified before
calls, with Rust 1.95.0 (`59807616e`, 2026-04-14), locked offline release Cargo and
`CARGO_PROFILE_RELEASE_DEBUG=1`; available Mach-O/debug identities are preserved.
Current build overrides are captured separately, not retrospective attestation
of unrecorded historical ambient variables. No product input changed after the
Task 7 accepted host/Linux source gates; subsequent harness/docs edits do not
pretend to change the measured binary. Original check60/read1k harness identity
is retained; later stages pin the corrected harness and its tests separately.

Host: Apple M4 Pro, 14 CPUs, arm64 macOS 26.6.2, 48 GiB RAM. Host load and OS page
cache are uncontrolled; the check60 start load averages were 12.918/6.526/6.658.
Every measured invocation is a fresh CLI process. Separate equivalent fixture
copies use seed 65, 2,000 bytes/fragment, 30 elements/file, disabled embeddings,
explicit isolated Git config and no real provider/model/paid session. Timed
repositories use SHA-1; separate correctness gates cover SHA-1 and SHA-256.
Membership stays 60 knowledge/60 source/300 edges or 1,000/1,000/5,000; all external
declarations and full capture membership remain. Inspect validates independent
path/range/hash plus retained bytes; traversal validates literal IDs/20 edges;
search validates exact IDs/flags; audits validate errors/counts. Copy inode refresh
is outside timing, then independent stable repositories are reused. Measured
projection revisions: check/check60: before [2], after [2]; inspect/reads1k: before [2], after [2]; search/reads1k: before [2], after [2]; traverse/reads1k: before [2], after [2]. They remain constant within each
ordinary series, avoiding hidden refresh costs.

Direct RSS raw samples include PID, parent, start timestamp, executable identity and
monotonic timestamps; 2 ms is the target interval, not a guaranteed sampling period.
An unsampled transient may be missed. Missing/identity-invalid evidence is null,
not zero. Four controlled allocation/sleeper/negative tests pass; a bytes-to-KiB
mutant fails. Initial Python fixture controls failed on its bin-to-app self-exec;
they are archived with the fixture correction to launch the stable app directly.
No sampler executable exception is added for Letopis. These single-call direct
observations are approximately 1.1–2.2 MiB higher after optimization; this is a
scoped observation, not a whole-stack memory comparison or a limit verdict.
Sample counts are before→after: check 4,776→11; inspect 1,636→222;
traverse 696→12; search 264→9. The largest observed inter-sample gap is 56.576 ms
(before check); all optimized gaps are at most 2.701 ms. Raw timestamps preserve
these sampling limits; a transient can be missed even when samples exist.

Exact commands and working directory are in control `*-command.json` plus stage
manifest settings; the shared root is
`/Users/zdkadiev/.codex/worktrees/issue-65/letopis`. Selected Python is
`/opt/homebrew/bin/python3.12 -B`. Commands are:

- Freeze: `tests/performance/compare.py --freeze-build --source <root> --evidence <root>/.artifacts/issue-67/after-20261031-task8`.
- Every comparison: `tests/performance/compare.py --stage <stage> --before <root>/.artifacts/issue-67/before-20261030-task1/build.json --after <root>/.artifacts/issue-67/after-20261031-task8/build.json --nodes <nodes> --scenarios <names> --timeout 120 --initial-timeout 600 --stage-seconds 1200 --evidence <new absolute path>` .
- Timing scenarios: check at 60; inspect, traverse and search at 1,000, both with `--pairs 10`. Initialization: check, reindex, status and refresh_bulk at 1,000 in one stage. Contention: search at 60. Profiles: check at 60; inspect, traverse and search at 1,000. Full exact argv is archived, without placeholder syntax.
- Focused test command: `/opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p test_compare.py -q`: 22/22 pass. Focused initialization RED: 3 tests, 1 expected failure; disabled-skip mutation: 1 expected failure; restored 22/22 GREEN. Raw logs contain scenario prints as well as unittest totals.
- RSS validation: `/opt/homebrew/bin/python3.12 -B .artifacts/issue-67/task8-control-20261031/direct_rss.py --test`: 4/4. Separate actual sampling omits `--test`; the saved source/hash and all raw samples are indexed.

## Acceptance ledger

| Criterion | Result | Evidence / limit |
| --- | --- | --- |
| Provisional 50% check 60 reduction | Met | 99.758%, n=10 each; no formal NFR verdict |
| Ordinary paired coverage and correctness | Passed | 80 measured successes; equivalent bytes/config, two warmups per side, stable revisions and independent oracles |
| Frozen baseline/optimized provenance | Verified | Sources, original receipts, binary/debug identities; local build provenance, not signed compiler attestation |
| No product Git helpers | Passed within recorded scope | Four release counters 0 after; Task 7 process/config/source audit and absolute-helper mutant; detached interval boundary stays explicit |
| Publication/full audit and maintenance 1k | Partially comparable | Publication n1 each; four baseline 120 s censors, after 3/3 each; no invented audit ratios |
| Reindex/reader schedule | Assessed | Kernel acknowledgement and positive overlap 3/3 each; exact lock wait unavailable |
| Separate profiles/counters | Recorded | 24 usable profiles, 24 plain controls, 8 valid counters; instrumented times excluded from ordinary series |
| Memory/resource evidence | Scoped only | 8 direct sampled calls plus wait4; Task 2 allocation/capacity evidence; no whole-stack/NFR claim |
| Final changed product gates | Accepted Task 7 evidence | Product unchanged; current harness 22/22 plus semantic mutation; see compatibility scope |
| Final whole-branch independent review | Pending | Controller-owned review/findings/rulings; this author does not certify it |

No duplicate unchanged Rust/native checks were run. Task 7 host snapshot plus
current test-only delta and final Linux native snapshot remain explicitly scoped
in [compatibility.md](compatibility.md); no historical harness, x86_64, NFS,
power-loss or real-provider suite is implied. Minor M3 same-handle mutation scope,
the tracing interval boundary and the pre-existing unverified direct-drain lock
concern remain in [review.md](review.md). Raw archives/profiles/payloads stay local;
[the canonical evidence index](evidence-index.json) points to hashed receipts,
frozen sources and runner scripts. Issue 65 evidence and unrelated staged/worktree
edits are preserved. No push, merge, GitHub update or issue closure was performed.
