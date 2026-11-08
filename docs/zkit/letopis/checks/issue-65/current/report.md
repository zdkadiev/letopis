# Issue 65: current-revision profiling and measured optimization

2026-11-05. The existing diagnostic runner was repaired for the current CLI and
rerun. One measured Git write bottleneck was optimized. Historical
[issue-65](../report.md) and [issue-67](../../issue-67/report.md) results remain
unchanged and do not describe this revision. No formal project gate is closed.

## Result

Repeated inspect has **78.04% lower median latency at 1k and 79.84% lower at 10k** in the
matched local experiment: ten valid ordinary executions per side/scale after two
conditioning executions per side. This is about 4.55× and 4.96× respectively.
Correctness oracles verify requested identity, independently frozen location and
SHA-256, retained snapshot bytes and complete projection node/source/edge counts.
Separate search oracles also verify clean freshness flags.
No paid provider or real model was invoked.

| Scenario / scale | Measured n per side | Before median, s | After median, s | Latency reduction | Max RSS before → after, MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| check, 1,000 | 10 | 0.207464 | 0.204706 | +1.33% | 19.00 → 18.44 |
| inspect, 1,000 | 10 | 0.405831 | 0.089132 | +78.04% | 14.80 → 13.92 |
| refresh_bulk, 1,000 | 3 | 0.469153 | 0.448755 | +4.35% | 21.97 → 22.36 |
| reindex, 1,000 | 3 | 0.424944 | 0.445516 | -4.84% | 21.67 → 21.86 |
| search, 1,000 | 10 | 0.031309 | 0.031403 | -0.30% | 13.42 → 13.50 |
| inspect, 10,000 | 10 | 3.774622 | 0.760819 | +79.84% | 19.58 → 18.84 |

Source: [ordinary comparison](reuse-comparison.json), [raw samples](comparison-samples.jsonl)
and [manifest](comparison-manifest.json). All 108 calls are valid: 92 measured
and 16 conditioning. Each execution is a fresh
CLI process; comparisons alternate before/after order. Copies, inode refresh and
fixture preparation are outside the timer. Stable read scenarios reuse separate
before/after repositories; mutation/full-index samples receive independent copies.
`reindex` is the historical scenario label; its actual command is `index --full`.
Search/check show no material change. The three full-index samples show a 4.84%
higher median, within a noisy small sample; this is not proof of no regression.
No p95/p99 is estimated from these sample sizes.

Max RSS is the maximum observed wait4 lifetime high-water mark for the direct
child and OS-accounted reaped descendants. It is not simultaneous tree peak or
whole-stack/model memory. CPU totals, block operations and individual durations
are retained in the raw records; OS blocks are not I/O bytes.

[Fresh-capture control](fresh-inspect.json), 10k, three alternating pairs, starts
each side from a projection that has never captured knowledge into Git. Before
median **5.579975 s**, after **4.794231 s** (14.08% lower). All six outputs are
valid. This separately exercises new objects and the extra hash/probe pass; it is
not pooled with repeated-capture measurements. First-capture observations from
the ordinary comparison remain conditioning, not ten extra successful samples.

## Build and experiment identity

- Before: clean product `52ed491a6688e0a4eee0cb5eceb3fc19f900465e`, binary SHA-256
  `c0ff5d131833a2a9072664a17d69d8ace80bee692bb6c510cf8da977ecf399c9`.
- After: measurement-only local clone commit
  `3b2e477cd2bb8bd204ab606a1566f32e713b4ac8`, binary SHA-256
  `77f8a66d0b0edf76799cb562537839d78fca20a2e78784c7dfe434ec9a8c7e8f`.
  All product input hashes match the actual changed checkout after mutations
  were restored. The main checkout's Git metadata is read-only in this session;
  it remains at the original HEAD with reviewable working-tree changes.
- Rust 1.95.0, `cargo build --release --locked --offline`, release debug symbols
  enabled. Both sides use the same compiler and dependencies. Full per-file
  source hashes are in [before](baseline-build.json) and [after](optimized-build.json)
  receipts; executables were copied away from mutable Cargo outputs.
- macOS 26.6.2 ARM64, 14 logical CPUs, 48 GiB RAM, Git 2.56.0, Python 3.12.0.
  CPU marketing name was unavailable under host permissions; disk model was not
  measured. Host load is recorded in manifests and comparison samples. This is
  not an isolated benchmark host or a production workload.
- Seed 65, 2,000 bytes per knowledge fragment, 30 elements/file, one source and
  five edges/knowledge. 1k: 34 Markdown files/5k edges; 10k: 334/50k;
  100k: 3,334/500k. Sources are sharded into 100-line files. Embeddings disabled.
  OS page-cache state is uncontrolled; no execution is labeled cold.

## Rust profile dumps and causal evidence

Samply 0.13.1 was attempted with save-only 1,000-Hz recording and symbols; it
failed with `Unknown(1100)`. Native `/usr/bin/sample` could not examine the exact
owned PID. Both failures are retained ([samply](native-profiler-failure.json),
[wall sampler](wall-profiler-failure.json)). No privileged retry was available.
These are unavailable profiles, not successful zero-sample recordings.

Instead, separate source copies contain temporary Rust `Instant` scope timers
and atomic aggregate call/time counters. [Exact instrumentation patch](timing-instrumentation.patch)
and [build receipt](profile-build.json) identify that diagnostic build. It emits
bounded JSON function dumps on process exit. The shipping product contains none
of this instrumentation. These are inclusive **wall-time** profiles, not CPU
sampling or exact blocked-time attribution. Parent/child scopes overlap: do not
sum their durations. The first parser wrapper hook does not cover the trait's
parser implementation; Markdown cost is therefore not separately established.

Initial 10k [inspect dump](profile-inspect-10k.json), one valid first capture,
5.041259 s end to end:

| Instrumented scope | Calls | Inclusive wall time, s |
| --- | ---: | ---: |
| InspectionService::inspect | 1 | 4.638516 |
| capture_file | 334 | 3.573900 |
| StagedObject::new_with_cleanup | 439 | 1.990830 |
| StagedObject::object_publish | 439 | 1.965350 |
| capture_sources | 1 | 0.794395 |
| assemble_tree | 2 | 0.230477 |

This capture includes 102 existing-object collisions. Source review shows every
repeat still stages, compresses, verifies, syncs and publishes all 439 objects.
The matched ordinary implementation comparison changes only that product path,
keeps workload/requests/oracles identical, and confirms its latency impact.

A separate repeat-capture instrumented control first captures outside its timer.
The [before dump](profile-repeat-before.json) contains **439 staged creations and
439 publications**; the [after dump](profile-repeat-after.json) contains **zero**
of each, **439 verified reuse calls and 439 file synchronizations**. The after
[patch](timing-instrumentation-after.patch) and [build receipt](profile-build-after.json)
identify that separate diagnostic build. Both retain
441 parent-chain synchronization calls and validate the same requested bytes and
graph totals. Their wall times are not the speedup estimate: these diagnostic
runs overlapped verification work. The ordinary comparison above did not.

Three alternating ordinary/instrumented controls were retained separately in
[overhead receipts](profile-overhead.json): medians 0.807848 / 0.570328 s.
Their inverse difference reflects host/load and measurement variability; it does
not show negative profiler cost or bound profiler overhead. Those controls also
overlap compilation. No diagnostic timing is pooled with ordinary latency.

## Change and remaining ranked costs

1. **Resolved repeated object staging in inspect.**
   [objects.rs](../../../../../../src/git/objects.rs) first asks
   [durability.rs](../../../../../../src/git/durability.rs) to confirm an existing
   loose object. It checks exact type and collision-aware content identity,
   synchronizes the actual backing file and parent chain, and checks cancellation
   before/after confirmation. Missing/packed-only objects use the original staged
   publication protocol. No persistent cache, public service contract, dependency,
   runtime or composition-root change was introduced. Existing duplicate-publication
   pathname/replacement limitations are unchanged; stronger power-loss or hostile
   replacement guarantees are not claimed.
2. **Remaining source observation cost.** A 10k
   [check dump](profile-check-10k.json) records 10k historical addressed reads and
   10k confined live captures: 1.780 / 1.444 inclusive seconds respectively in an
   instrumented 3.528-s call. `observe_sources_batch` totals 3.236 s across 40
   batches, containing those scopes. These counts/time locate work, but do not
   prove that caching or coalescing live captures would preserve freshness.
   Any optimization must preserve edits between observations; no live cache was
   added. This remains a measured cost with an unimplemented optimization hypothesis.
3. **100k first publication remains unresolved.** The one attempt timed out at
   120.012554 s, with 105.361411 CPU seconds and 363,347,968-byte (~346.52 MiB)
   wait4 peak RSS. [Receipt](survey-100k-timeout.jsonl). This is a censored attempt,
   not a completed load or an initialized-query observation. Exact storage/SQL/
   provenance attribution at this scale is not established. No automatic retry
   with a larger limit or extrapolation from 10k was made.

A separate [1k full-index overlap](index-overlap.json) observes the owned writer's
index lock and validates both commands. Reader duration is 0.056375 s alone and
0.778576 s with the writer, with 0.736306 s CLI lifetime overlap. This supports an
application-lock head-of-line effect; it does not quantify SQLite lock waits.
A 60-node packing control was also retained; its noisy n=3 timings do not confirm
a current per-file latency slope. It observed zero Git subprocesses.

Local [follow-up issue drafts](follow-ups.md) specify the remaining hypotheses,
proposed improvement criteria, risks and regression checks. They were not published.

## Current baseline coverage

These single-revision survey medians precede the optimization; do not relabel
them as optimized measurements. Each cell is seconds / measured n.

| Scenario | 1k | 10k |
| --- | ---: | ---: |
| search | 0.0424 / 10 | 0.0851 / 3 |
| inspect | 0.3985 / 10 | 3.5906 / 3 |
| traverse | 0.0353 / 10 | 0.1059 / 3 |
| check | 0.1969 / 10 | 2.0079 / 3 |
| status | 0.1790 / 10 | 1.9564 / 3 |
| refresh_one | 0.0657 / 10 | 0.2853 / 3 |
| refresh_several | 0.0752 / 10 | 0.3006 / 3 |
| refresh_bulk | 0.4395 / 10 | 5.9042 / 3 |
| reindex | 0.4200 / 10 | 5.4055 / 3 |

First publication: 0.441568 s / n=1 at 1k; 4.539715 s / n=1 at 10k.
Initial 60-node smoke reproduced the removed-command failure; that failed sample
remains retained. The repaired 1k/10k surveys validate all nine scenarios.
100k initialized scenarios are **not assessed** because publication timed out.

Real hybrid, real-model/query/document provider time, model RSS, fake-provider
coverage/drain, parallel drain, Linux/x86_64 and production traces are not assessed
in this session. Loopback binds and process inspection are blocked, preventing the
controlled fake-provider/drain tests. Exact SQL query counts, SQLite lock waits,
product parsed/hashed bytes, I/O bytes and simultaneous whole-stack peak remain
unavailable. No model-free result is substituted for those metrics or for #35.

## Verification and review

- Five new Rust behavior/failure tests cover both object formats, loose tree/blob,
  packed fallback, absent objects, corrupt and decodable wrong-identity data,
  safe diagnostics, cancellation and object/directory sync failure propagation.
- Four hand mutants were rejected: omit identity verification, initial cancellation,
  file synchronization, and parent synchronization. All source was restored;
  [receipts](rust-mutations.json). Sync failure checkpoints live inside the actual
  shared operations, so omitting the caller operation omits the checkpoint too.
- Three harness regressions failed before implementation, then passed; three
  hand mutants were rejected ([receipts](harness-mutations.json)).
- `cargo test --locked --offline --lib git::`: **52 passed** on the candidate;
  `cargo test --locked --offline --release --lib git::`: **52 passed** on the
  actual changed checkout. Strict Rustdoc passed. All-target Clippy passed on the
  candidate; the final root format, all-target check, Clippy and fixed-value lint
  steps also passed. Exact commands are recorded in [verification](verification.json).
- Generator: **25 passed**. Full Python performance suite: **84 run, 72 passed,
  4 permission errors, 8 explicit integration skips**. The four errors are the two
  fake-provider bind tests and the measure/window `ps` cleanup tests. They were
  already failing before product changes; no failure was converted to a skip.
- `make verify` was run on the original and actual changed checkout. Original:
  **558 passed, 33 failed, 9 ignored** in the library suite, with socket/process
  permission failures. Final rebuilt run: **563 passed, the same 33 failed, 9 ignored**. Every failed
  test name and exact command are retained in [verification](verification.json);
  full verification is not claimed. An earlier run picked up a deliberately
  mutated debug executable from the shared Cargo target. Focused replay confirmed
  the stale artifact, forcing source recompilation fixed it, and the final suite
  contains only the original permission failures. Both runs are retained; future
  mutation work must use a separate Cargo target directory.
  Storage/native runners and historical proof harnesses were not rerun: their
  implementation/build/vendor inputs did not change. The root no-helper integration suite passed **4 tests**, with the loopback
  provider case explicitly filtered; Linux syscall tracing was not run.
- Independent code review found one Important diagnostic leak (raw decoder cause).
  A canary Display/Debug regression reproduced it, then the original safe mapping
  was restored. Scoped review of that fix and shared-sync extraction found no
  remaining Critical/Important issues. Directory-entry observations are described
  as residual-entry checks, not a proof that no transient stage existed.
- Independent data review checked all 108 comparison calls, all table arithmetic,
  profile counts and 106 optimized product input hashes. Minor wording/oracle-scope
  corrections and raw-sample/build links were applied. The final actual root release
  was rebuilt and smoke-tested with valid inspect output and no profiling stderr
  ([receipt](final-product-smoke.json)).

[Evidence index](evidence-index.json) hashes durable receipts and raw run records.
Raw corpora and executables remain ignored; recorded deterministic inputs and
archived comparison drivers reproduce the experiment with new output paths.
See [runner README](../../../../../../tests/performance/README.md),
[design](../../../../../superpowers/specs/2026-11-05-issue-65-refresh-design.md), and
[execution record](../../../../../superpowers/plans/2026-11-05-issue-65-refresh.md).
