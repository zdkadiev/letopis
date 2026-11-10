# Issue 108: measured historical provenance results

## Stable measured results

Measured on 2026-11-10 on an Apple M4 Pro/macOS shared host with synthetic seed-65 corpora and embeddings disabled. The two immutable release builds use rustc 1.95.0, locked/offline dependencies and release debug=1. OS page cache and unrelated host load are uncontrolled. These measurements establish neither production nor minimum-hardware acceptance.

Each ordinary row has two conditioning calls per side followed by three successful alternating pairs. Wall, CPU and RSS below come only from ordinary CLI calls, never Samply or the instrumented build. CPU/RSS are wait4 direct-child plus OS-accounted reaped-descendant scopes; RSS is a lifetime maximum, not simultaneous process-tree peak. Portable I/O-byte counters were unavailable.

| Nodes | Variant | Command | n before/after | Wall s before → after | Speedup | CPU s before → after | Peak RSS MiB before → after |
|---:|---|---|---:|---:|---:|---:|---:|
| 10000 | distinct | check | 3/3 | 10.228352 → 10.345796 | 0.989x | 9.228833 → 9.274468 | 43.88 → 44.22 |
| 10000 | distinct | status | 3/3 | 10.243373 → 10.007750 | 1.024x | 9.221623 → 9.153257 | 39.20 → 40.44 |
| 10000 | shared | check | 3/3 | 1.200717 → 0.972963 | 1.234x | 1.152764 → 0.923855 | 35.03 → 35.47 |
| 10000 | shared | status | 3/3 | 1.153359 → 0.925199 | 1.247x | 1.106325 → 0.882078 | 32.94 → 33.59 |
| 100000 | distinct-sharded | check | 3/3 | 50.399565 → 51.390430 | 0.981x | 39.937657 → 40.002063 | 238.86 → 247.98 |
| 100000 | distinct-sharded | status | 3/3 | 50.054357 → 50.038821 | 1.000x | 39.333133 → 39.367600 | 224.09 → 232.09 |
| 100000 | shared | check | 3/3 | 11.729340 → 9.422573 | 1.245x | 11.580312 → 9.197418 | 199.05 → 203.95 |
| 100000 | shared | status | 3/3 | 11.311962 → 8.966203 | 1.262x | 11.201395 → 8.831660 | 197.98 → 201.02 |

Both sides validated exactly N knowledge nodes, N external edges, and 61 distinct source selectors for shared or N for unique-path variants. Status returned revision 1 and embedding backlog 0. There are 48 successful ordinary measured calls across the eight rows; profile-stage controls are separate. Shared speedups are 1.234–1.262x. Small samples do not support p95/p99 or significance claims.

Observed control regressions are retained: 10k flat distinct check wall +1.15%; 100k sharded check wall +1.97% and CPU +0.16%. Sharded status is near parity. Median whole-child RSS rises by 3.82% (9,568,256 bytes) for sharded check and 3.57% (8,388,608 bytes) for sharded status; shared 100k check rises by 2.46% (5,144,576 bytes). This is a measured memory tradeoff, not zero regression and not an allocation/leak attribution.

**Original flat 100k remains unavailable:** baseline first publication hit its original 600 s limit at 600.007386 s, using 588.890940 s CPU and peak RSS 155,353,088 bytes. No conditioning/measured samples exist for that input. Candidate setup was observed once, then intentionally cancelled at 278.272005 s; it is observer-affected interruption, not a timeout or product failure.

The authorized supplemental `distinct-sharded` keeps the same 4,500-byte/100-line source content, N distinct whole-file sources, N knowledge nodes and N edges, but places 100 files per leaf directory (1,000 leaves at 100k). Source paths, hierarchy and source-tree OID change. Before/after compare only within this variant. Its success does not turn the original flat timeout into success or establish an isolated across-variant speedup. Publications took 217.779830 / 215.360522 s; they are excluded from ordinary medians.

### Separate CLI-only profiles

Each cell below pools the observed CPU deltas of three captures for the same scale/command/side; every capture was restricted to the PID whose processName is `letopis`. All 24 captures are valid, with raw gzip, presymbolicated sidecars and explicit `cli-stacks.json` retained. Inclusive sampled scopes overlap and must not be added or treated as latency shares. Absence of a named/inlined function frame does not mean zero executions.

| Nodes | Command | Usable stacks before/after | Git object verification % before → after | Filesystem % before → after | SHA-256 % before → after |
|---:|---|---:|---:|---:|---:|
| 10000 | check | 3353/2769 | 15.51 → 0.78 | 78.43 → 92.07 | 0.54 → 0.79 |
| 10000 | status | 3324/2649 | 17.92 → 0.76 | 75.73 → 91.64 | 0.43 → 0.56 |
| 100000 | check | 33861/28057 | 16.43 → 0.48 | 77.22 → 92.23 | 0.66 → 0.83 |
| 100000 | status | 32741/26742 | 17.04 → 0.60 | 76.45 → 91.89 | 0.70 → 0.68 |

The remaining shared-workload hotspot is fresh filesystem capture. Historical lookup frames disappear from these sampled candidate profiles, but actual candidate counters below show nonzero grouped reads. Software SHA-256 is a configuration fact and a potential future option, not the main remaining cost in this shared workload.

### Actual candidate counters

An isolated instrumented candidate source copy measured the following complete CLI calls. Decode counters sit immediately after actual uncached ODB lookup/decode, with blob and tree kinds counted separately. They include all CLI Git work, not only a provenance group. Group reuse belongs to one observe_sources_batch call; diagnostics use chunks of 256 knowledge nodes, so an entire CLI invocation has many groups. No baseline operation counters are invented from these candidate counts.

| Nodes / variant / command | Accepted batches | Blob/tree decodes | Historical SHA calls / bytes | Live SHA calls / bytes |
|---|---:|---:|---:|---:|
| 10000 / shared / check | 40 | 41/164 | 2395 / 285,975 | 10000 / 3,234,375 |
| 10000 / shared / status | 40 | 40/160 | 2395 / 285,975 | 10000 / 3,234,375 |
| 100000 / shared / check | 391 | 392/1568 | 23851 / 2,815,200 | 100000 / 32,343,750 |
| 100000 / shared / status | 391 | 391/1564 | 23851 / 2,815,200 | 100000 / 32,343,750 |
| 100000 / distinct-sharded / check | 391 | 100001/401395 | 100000 / 450,000,000 | 100000 / 450,000,000 |
| 100000 / distinct-sharded / status | 391 | 100000/400391 | 100000 / 450,000,000 | 100000 / 450,000,000 |

Historical exact-selector SHA deduplication already existed before this change. The new implementation removes repeated verified Git reads and boundary searches across distinct ranges of one accepted group; a measured baseline SHA-call reduction is not claimed. Task 3 separately proved one actual blob decode, three tree decodes and three historical SHA calls over 20 bytes in its real loose-object fixture; that existing evidence was reused, not repeated or presented as a whole-CLI count.

### Candidate costs (CPU / wall seconds)

Representative status calls show the disjoint SHA/boundary/live-capture scopes and inclusive Git lookup below. CPU uses CLOCK_THREAD_CPUTIME_ID for the calling thread and wall uses Instant, unlike ordinary whole-child wait4 CPU. Nested Git lookup, lookup/decode, object checksum and outer provenance scopes overlap; do not sum them. Full check/status counters and intervals are retained for all six calls.

| Nodes / variant | Historical Git lookup inclusive | Historical boundaries | Historical SHA-256 | Live file capture | Live boundaries | Live SHA-256 | Metadata planning |
|---|---:|---:|---:|---:|---:|---:|---:|
| 10000 / shared | 0.003460 / 0.003480 | 0.000203 / 0.000207 | 0.000786 / 0.001021 | 0.763309 / 0.765095 | 0.004735 / 0.005749 | 0.006812 / 0.007808 | 0.001476 / 0.001482 |
| 100000 / shared | 0.036222 / 0.036315 | 0.002171 / 0.002215 | 0.007779 / 0.010127 | 7.992005 / 8.061477 | 0.049058 / 0.059578 | 0.070502 / 0.080844 | 0.021360 / 0.021426 |
| 100000 / distinct-sharded | 24.667298 / 25.128027 | 0.014948 / 0.024856 | 0.784658 / 0.795929 | 12.980311 / 22.733442 | 0.010463 / 0.021029 | 0.819534 / 0.831699 | 0.039965 / 0.040023 |

At 100k sharded status, actual ODB lookup/decode CPU was 2.434614 s for blobs and 15.580807 s for trees; object checksum CPU was 5.540084 s nested within the inclusive historical Git lookup. Actual decoded content was 450,000,000 blob bytes and 4,145,702,294 tree bytes. For 100k shared status, blob/tree lookup-decode CPU was 0.008447/0.034115 s; object checksum CPU was 0.004185 s. These measured nested scopes are not an additive decomposition.

Boundary byte counts mean input-buffer bytes, not bytes necessarily scanned: whole-file-only groups return without scanning line boundaries. At 100k shared status there were 23,851 grouped selector results, 391 boundary preparations and 100,000 fresh live selections; historical/live boundary input bytes were 1,759,500 / 450,000,000. At 100k sharded status each side had 100,000 whole-file selections over 450,000,000 input bytes. Hash-byte totals in the counter table count exact addressed bytes.

Instrumentation adds clock reads, counter aggregation and maxima updates. Empty-scope calibration measured roughly 0.09–0.23 microseconds CPU and 0.19–0.50 microseconds wall per scope, not subtracted; short hashes/boundary operations are especially affected. Probe process timings are not ordinary samples or bare-operation benchmarks.

Measured plan/result maxima are 62 entries / 4,042 logical address bytes for shared at both 10k and 100k, and 257 entries / 19,240 address bytes for 100k sharded. All calls emitted accepted-plan markers; no CLI fallback occurred because 256-node chunks fit below the 4,096-entry and 1 MiB admission caps. Fallback correctness is covered by the focused budget tests and full shipping suite, not claimed as an observed CLI performance path. Metadata planning CPU was about 0.021 s shared / 0.040 s sharded at 100k, including probe overhead.

Ownership review found only borrowed group addresses and compact hash/None results in batch state. The returned historical blob vector ends with group preparation before the next live capture; each batch owner is dropped after its 256-node chunk. Existing ObjectReader buffer reuse is separate and unchanged. Stable per-batch maxima across scales and budget/lifetime tests show no new unbounded retention; they do not explain every byte of process RSS or prove a general allocator bound.

Cargo feature evidence shows sha2 0.10.9 default/std without asm. Its dependency source selects the software SHA-256 backend on aarch64 in that configuration. The Apple M4 Pro reports hw.optional.arm.FEAT_SHA256=1, but this does not mean the shipping sha2 backend uses hardware SHA256. Features and algorithm were not changed.

<!-- issue108-stable-results-end -->

## Build and verification provenance

| Build | Source commit | Binary SHA-256 |
|---|---|---|
| Before | 5a6591bc9ded98a1ee9916b325133bbfa056cf5f | eedaaab966612d01a00058bf00260aa7f2e2d6706bdb51cdb159ea90b98dda7e |
| Candidate | 557d8efce1adc1b8696fde76b2ee3a9d6c8edc29 | 50a1c9016ea2d4e6decb5661431de540d37e95ff960852d5334f31e0e5635b99 |

Immutable source archives, per-input hashes, original release receipts, binaries and available symbols are in `.artifacts/issue-108/{before,after}-build/`. Both use cargo build --release --locked --offline --message-format=json with CARGO_PROFILE_RELEASE_DEBUG=1 and the same pinned rustc. The invocation-local freeze wrapper only redirects the build target; archive/hash verification is unchanged. Product inputs were clean at freeze; the preserved dirty knowledge/documentation delta is outside the product-input manifest. Later harness/report commits do not change the frozen Rust inputs.

Full shipping CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target CARGO_BUILD_JOBS=2 make verify V=1 exited 0 on final Rust state 557d8ef. Debug plus example: 1,297 passed, 0 failed, 11 ignored; release: 1,287 passed, 0 failed, 11 ignored. Fmt, all-target check/Clippy with -D warnings, fixed-value lint, rustdoc with -D warnings and doc tests passed. The previously failing FIFO watchdog passed unchanged in debug and release. No watchdog was skipped or relaxed, and no shipping Rust fix was made by Task 5. Storage/native/build/vendor are unchanged, so no extra native runner was required. Exact output and parsed totals: ignored `task-5-evidence/verify-initial.log` and its summary JSON.

The isolated probe used its own `.artifacts/issue-108/instrumentation-target`, never the canonical target. Binary SHA-256 2a6ae2def5cba5a132176d5f994cecd1a66b5993b9a83fbd4e3461f18655f8ac; patch SHA-256 979b0c083b3ba7c8223c7b08107ed4f532fb8e75decbe3b20ed2b66fcaa49098; source-manifest SHA-256 bd6e21699a1c3326ffa946b9f02b4ca133794985216e7ff119781031620aa21b. Exact source, patch, build log, generated dSYM, artifact hashes, raw measurements and counters are in ignored `task-5-evidence/instrumentation/`. All six calls passed independent CLI/count validation. The existing Task 3 real-decode evidence retains its own earlier instrumented identity and is not relabelled as this build.

## Scheduling, censored setup and remaining limits

The original four timing campaigns retained the 120 s call, 600 s first-publication and 1,200 s stage limits. A failed side stopped without retry. The original flat 100k baseline timeout and candidate observer-affected cancellation are preserved in `timings-100000-distinct/`; neither supplies an ordinary latency or speedup.

One authorized five-second native sample attached only after exact owned PID/start/ancestry and frozen executable SHA checks. It observed 3,496 of 3,860 main-thread stacks under blob lookup → tree read, 2,977 in its checksum branch (ObjectHasher → sha1_checked), and 492 in lookup/decode. Counts are inclusive wall-stack observations from one window, not command-wide CPU proportions. The flat source tree has 100,000 entries / 4,388,890 bytes, OID f35cf3d0167163719df53a026b6394289139c1a8. This directly locates repeated wide-tree verification as the setup hotspot. Raw sample, identities, symbols receipt and exact cancellation live in ignored `task-5-evidence/flat-100k-setup-sample/`.

Supplemental sharded check required about 433 s publication plus about 500 s check calls. About 1,000 s of both commands plus setup would exceed one stage limit. Controller split by command: a kqueue watcher waited for all six successful check records and their post-fsync log writes, then SIGINTed only the verified owned campaign before any status attempt. The aggregate stage is intentionally interrupted, with no check sample lost. Status used a fresh bounded stage and fresh required conditioning/three pairs on the same initialized fixtures, with no republication or check rerun. Its manifest records continuation_of, all actual input/config/ignore hashes, tree identity/source endpoint bytes, counts and revision 1.

Profiles similarly reused verified successful shared fixtures under a separate archived invocation-local driver and unchanged helper measurements, with no cold-cache claim. Fresh conditioning and three alternating ordinary/Samply pairs per side/command were retained. No ordinary call overlapped compilation, tests, profiles or another instrumented product call; the one native observer attached only to the excluded flat candidate setup. Profile controls remain separate from ordinary results.

The original flat 100k acceptance remains partial. The agreed representative 100k unique-path control passed under a normal hierarchy; it does not erase the wide-tree limitation. Search/traverse singleton observations and compare_revision were not optimized. No claim is made about real-provider, production, minimum-hardware, arbitrary-corpus scaling or p95/p99 performance.

## Harness, knowledge and self-review

Controller expanded Task 5 ownership to the minimal distinct-sharded harness addition, committed as 81e96a8. TDD RED: absent variant rejected by real fixture API and CLI. Focused GREEN: 10 tests; full Python harness suite: 94 tests in 25.475 s, OK with 8 optional integration skips. Four mutants (wrong leaf width/path, missing recursive hashes, missing CLI choice) were rejected; exact code restored and 10 tests passed again in 6.793 s. The new 201-file fixture checks leaf boundaries, unique whole-file selectors, deterministic hashes/tree, actual Git tree bytes, counts and navigation. No shipping Rust behavior changed in Task 5.

Non-product setup errors were retained: an initial test-write invocation used a wrong relative path and wrote nothing; it is not TDD RED. Probe-source rustfmt first tried to follow a cfg(test) path omitted from the product archive; its failed source copy/log were preserved and skip_children fixed preparation before any probe build. The instrumentation wrapper initially expected an optional dSYM absent in the unpacked debug build; before any product invocation, exact probe symbols were generated once with dsymutil. No failed/timed-out heavy product call was retried.

The original user-checkout performance knowledge delta was saved byte-for-byte plus patch and carried into this worktree unchanged; original checkout/config/cache were untouched. Old created_at/source_snapshot values remain historical. New measured knowledge, exact source-basis/range checks and CLI validation are recorded below after completion.

Self-review checked source identities, valid/failed attempt classification, no sample pooling across phases/variants, CLI-only profile PID attribution, inclusive overlap, actual/inferred counter distinction, 256-node batch scope, fixed cache admission/lifetime, fresh live capture, backend configuration, RSS tradeoff, preserved knowledge delta and owned-file commits. Independent Task 5 and whole-branch review belongs to the controller and is not claimed complete here.

## Execution rulings preserved

Ruling: Continue independent #108 work with this named pre-existing baseline failure retained, rather than change unrelated filesystem code or claim baseline green — final verification must report whether it persists — cost is incomplete baseline cleanliness, no hidden product-code fix.

Ruling: Freeze baseline using an invocation-local build_release target override, retaining the wrapper in evidence and the unmodified freeze archive/hash verification — avoids duplicating dependency builds in the managed worktree — no product behavior cost; build-cache identity is recorded.

Ruling: Task 1 freezes the original binary and completes harness tests plus 30-node CLI smoke; 10k/100k baseline timings run as matched pairs in Task 5 instead of duplicate baseline-only series now — source-identical baseline is captured before Rust edits and all performance acceptance still applies — cost is no full-scale timing until candidate exists.

Ruling: Intermediate Tasks 2–4 use focused behavior checks and scoped lint, preserving any already-started broad-run logs; defer the global suite to final Task 5 make verify — avoids repeated compilation of unrelated acceptance binaries during unstable host periods — integration-wide failures may be detected later, and final verification remains mandatory.

Ruling: Expand current Task3 implementer ownership only to correct the Task2 test-target lint in source_test.rs, preserving domain production behavior — one implementer avoids an overlapping fixer and closes final-Clippy failure early — cost is a small cross-task test-only fix included in Task3 review.

Ruling: Expand Task4 ownership narrowly to rustfmt-only import correction in Task3 provenance_test.rs found by cargo fmt --check — fixes final formatting gate without another implementer — cost is a test-only formatting hunk outside the original integration files, no runtime behavior.

Ruling: After the already-started Task4 legacy mutation wave finishes (including its final planned empty-selector mutant), isolated Cargo source-copy builds require their own target directory; package-only clean of shared letopis artifacts followed by canonical focused/full rebuild restores shipping provenance — avoids mutant artifact reuse — costs rebuild time, not source or dependency deletion.

Ruling: Resolve plan's ambiguous 'singleton' against stronger approved spec: observe_sources API stays legacy, but observe_sources_batch must group accepted plans even for one request containing multiple ranges; remove len>1 guard with a failing one-request test — preserves one-blob-read promise — costs small metadata planning overhead for single-request batches. Task4 ownership narrowly expanded to this guard and focused unit regression, with independent review before acceptance.

Ruling: Take one ~5s supplementary native sample of the already-running, identity-verified owned candidate setup PID, without another product invocation — obtains hotspot evidence without retrying censored heavy input — candidate setup timing is observer-affected and cannot be used as ordinary/comparable latency; prior ordinary samples remain untouched.

Ruling: Safely cancel already-observed candidate flat setup (explicit cancelled/observer-affected, not timeout or source failure) and add one supplemental distinct-sharded100k control with100files/leafdir, same bytes/counts and unique whole-file bindings — hierarchy reduces verified-parent-tree cost so scoped optimization can be assessed at100k — costs an additional separately labelled workload; cross-layout timings are not equivalent and original flat failure is retained. Task5 harness ownership minimally expanded with TDD. No Rust feature/dependency change or automatic retry of flat input.

Ruling: Preserve completed sharded check series, then continue only remaining status work in a separate phase on verified same initialized inputs/frozen binaries, with archived continuation driver and explicit continuation_of — fixes stage orchestration budget without increasing120s/call or1200s/phase or republishing — costs longer total multi-phase experiment; scheduling-censored attempts stay separate and no valid check samples are repeated.

Additional rulings during Task 5:

- Reuse Task 3 real decode multiplicity evidence; measure remaining candidate cost only, without a second instrumented baseline. Cost: no measured baseline operation-count comparison, clearly labelled.
- Observe the already running owned flat candidate setup once, then intentionally stop it via verified safe cancellation; no retry. Cost: observer-affected candidate setup, excluded from ordinary latency.
- Add one separately labelled 100k distinct-sharded control after concrete wide-tree hotspot evidence. Cost: changed paths/tree shape, so it cannot complete or be pooled with the original flat input.
- Split the sharded workload by command after real per-call cost showed the aggregate orchestration budget could not fit setup plus all calls. Cost: explicitly interrupted original stage and a provenance-linked status-only continuation, retaining every per-call/stage bound.
- Reuse verified initialized shared fixtures for separate profiles while retaining conditioning and all pairs. Cost: warm initialized state, no cold-cache claim.
- Retain the 3–4% whole-child RSS increase as a measured tradeoff; no extra heavy run solely for it. Use the already planned probe maxima/costs and source lifetime review; do not attribute all RSS delta to cache metadata.

## Acceptance and controller review handoff

| Task 5 item | Evidence/status |
|---|---|
| 5.1 full shipping verification | PASS, final Rust state, unchanged FIFO watchdog |
| 5.2 immutable candidate | PASS, source/binary hashes and matching toolchain/build options |
| 5.3 original ordinary matrix | 10k shared/distinct and 100k shared PASS; original flat 100k unavailable; approved supplemental sharded representative control PASS |
| 5.4 profiles/cost/backend | PASS, 24 valid captures + six actual candidate cost calls + backend evidence |
| 5.5 report and limits | Recorded with regressions, RSS rise, censors and scope qualifications |
| 5.6 knowledge | PASS, verified complete source tree and stable addressed ranges; check/search/inspect/read-back |
| 5.7 independent final review | Controller-owned, pending |
| 5.8 local commits/handoff | Harness commit 81e96a8; evidence/knowledge commit delivered in task-5-report.md; controller final user handoff pending |

No push, merge or issue closure was performed. Exact invocations and raw-output locations are in [commands.md](commands.md). All raw campaign/probe artifacts are local ignored evidence; the committed report preserves the measured results and execution decisions, not portable copies of every raw capture.

## Knowledge verification receipt

The new element is `letopis/development/performance-bottlenecks.md#historical-range-reuse-2026-11-10`. Its verified complete source tree is `cd688426bf88773c9d3fc3fb3f658a0a31302cd9`; bindings include stable report lines 1–77 and the exact current provenance/batch-owner bytes. A separate temporary index left the real index unchanged. The old knowledge delta remains a byte-identical prefix with its original metadata.

Using only the worktree cache with embeddings disabled, `check` exited 0/ok:true with `errors:[]`; search returned the new element at revision 2 with `flags:[]`. `inspect` exited 0 and returned snapshot `477b390f669446f42feb496cc329ccf4b0138b21`; all five knowledge/source descriptors were read from that same tree, sliced using returned inclusive lines and matched their returned SHA-256 hashes. Full requests/responses, basis/range hashes and returned-tree bytes are retained in `task-5-evidence/knowledge/`. Old sections keep their original basis and existing source_changed search flags; no new verification result is attributed to them.
