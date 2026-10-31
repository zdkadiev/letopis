# Issue #49 — bounded parallel drain

2026-10-31. Implementation and model-free verification; **real-provider acceptance remains open**.

[Issue #49](https://github.com/zaurkadiev/letopis/issues/49) ·
[accepted specification](../../../../superpowers/specs/2026-10-31-issue-49-parallel-drain-design.md) ·
[implementation plan](../../../../superpowers/plans/2026-10-31-issue-49-parallel-drain.md) ·
[D-107](../../decisions.md#D-107) · [measurement instructions](../../../../../tests/performance/README.md#bounded-parallel-drain-comparison-49).

## Product behavior and boundaries

One process still owns `embed.lock`. `DRAIN_WORKERS = 4` at the top of
`src/service/embedding/drain.rs` bounds worker threads **and all submitted results
not yet handled**, including completed responses. A coordinator performs storage,
file reads and Markdown preparation; only provider work crosses to workers through
`Arc<dyn EmbeddingProvider + Send + Sync>`. The existing synchronous ureq client,
60-second per-attempt deadline, query path, finite retry policy and JSON/config
schemas remain. `threadpool =1.8.1` is the only new direct dependency; locked
transitives are num_cpus 1.17.0 and hermit-abi 0.5.3.

The coordinator excludes active hashes, reserves attempts durably before dispatch,
and consumes whichever result arrives first. One short transaction publishes each
result or failure before freeing its slot. HTTP runs outside transactions; ready
responses do not wait for their batch peers. Empty selection is final only with no
pending jobs. Normal/error/unwind cleanup joins admitted jobs before releasing the
lock. Programming panics remain panics; forced process death relies on OS/SQLite
cleanup and leaves spent attempts durable. A provider retry delay may temporarily
pause the coordinator, including handling other completed results.

## Reproducible evidence and provenance

Ignored raw artifacts are under `.artifacts/issue-49/`; preserve that directory to
inspect local receipts. No historical evidence was overwritten.

| Variant | Product source | Frozen receipt | Binary SHA-256 |
|---|---|---|---|
| A | `b7f627d1d63d2d7b72989b733838a55925947c17` | `builds/before/build.json` | `010036d3498a365768ee3c6c1aeac0d5c4ab201d58fa817012279acf6d4a6791` |
| B | `a4d458ab135c341de559cd93aa85337e9d1975b2` | `builds/one-worker/build.json` | `6ae202c87eb998f53872297c0abf12051175758d8b9e2c2c705f1d1cf4261287` |
| C | `68c9861458912cda070bdc44c24a44bc80ce9c74` | `builds/after-corrected/build.json` | `7f18c20fef8c679227edc59db2a6090be4eb299b47c51d52bc847cb942a187fa` |

A is branch base `b7f627d1d63d2d7b72989b733838a55925947c17`. B is Task-3 product
with the single `DRAIN_WORKERS: 4 → 1` evaluation patch preserved as
`builds/one-worker.patch`; C is unchanged Task-3 product (`68c9861`). Source archives,
original release receipts, artifact/symbol hashes and the compiler are pinned.
`builds/correction.json` rejects a provisional C executable reused from B by shared
Cargo-target freshness. No timing campaign used that invalid pin; a forced package
rebuild produced the distinct C hash above. Disposable source target symlinks were
removed. Frozen manifests and actual observed concurrency both guard this mistake.

`campaign-4/manifest.json` captures host, exact fixture/config hashes, all harness
source hashes and the native observer binary/source. 21/21 cases are valid: one
conditioning plus five measured cases per variant, followed by one query-overlap
case each. Inputs are byte-identical across all cases. Setup, observation and
build time are outside drain timing. Fresh standalone projections contain 64
unique ASCII texts (roughly 120 repeated words each) and four extra same-text nodes.
The concurrent loopback provider returns independent literal three-component
vectors with 25 ms delay; overlap cases use 100 ms delay. No external forwarding,
real model or paid session is involved. Trials run interleaved A/B/C on the same
Apple M4 Pro / macOS 26.6.2 ARM64 host. OS page cache and unrelated host load are uncontrolled.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/drain_compare.py \
  --before "$PWD/.artifacts/issue-49/builds/before/build.json" \
  --one-worker "$PWD/.artifacts/issue-49/builds/one-worker/build.json" \
  --after "$PWD/.artifacts/issue-49/builds/after-corrected/build.json" \
  --evidence "$PWD/.artifacts/issue-49/campaign-4"
```

The command refuses reusing an existing directory; choose a new suffix to rerun.
Campaign-1 stopped during setup (create-only writer used for mutable private
configuration). Campaign-2 retained incomplete vector coverage: implicit-close
HTTP/1.0 fixture responses raced pooled connection reuse. An isolated shared-ureq
probe reproduced 245/256 delivered requests; explicit `Connection: close` made
256/256 pass. This framing fix is confined to the fixture, not product transport.
The incorrect campaigns, probe source/logs and their failures remain retained.
HTTP/1.0 peers that close without an explicit header remain a transport compatibility
risk; no arbitrary transport change or extra retry was added here.

## Controlled results

| Variant | Valid measured calls | Unique vectors / nodes per call | Median seconds (range) | Vectors/s | Max document calls | HTTP attempts / errors per call | Drain RSS median / max MiB | Median CPU seconds | Overlap query median ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A — sequential base | 5/5 | 64 / 68 | 2.4403 (2.3998–2.8576) | 25.29 | 1 | 64 / 0 | 12.84 / 12.89 | 0.1005 | 135.37 |
| B — new scheduler, 1 worker | 5/5 | 64 / 68 | 2.4355 (2.3921–2.5021) | 26.21 | 1 | 64 / 0 | 12.91 / 12.98 | 0.1091 | 130.84 |
| C — new scheduler, 4 workers | 5/5 | 64 / 68 | 0.6952 (0.6838–1.7804) | 70.44 | 4 | 64 / 0 | 14.78 / 14.83 | 0.0949 | 127.78 |

Each variant produced 320 verified unique vectors across five measured calls,
with 64 document requests and zero failed hashes per call. No retries were needed
in this zero-retry fixture. C/A median speedup is **3.51×** (throughput ratio 2.79×);
B/A median ratio is 0.998, separating scheduler overhead from parallelism.
One C sample took 1.7804 s while the others took 0.6838–0.6980 s;
that slower sample remains in the range and throughput denominator. The cause is
not established; host load/page cache are uncontrolled. Earlier valid campaign-3
reported 3.67× median / 3.65× throughput. Campaign-4 was repeated after extending
the watchdog to setup, not selected to maximize speedup. These are observations of
hiding synthetic HTTP wait, not a real-model speedup forecast.

Every overlap case ran three actual vector-only search calls, returned the exact
canary ID, and recorded all three during document inference. Maximum total HTTP
concurrency was A/B/C = 2/2/5, while document concurrency stayed 1/1/4. These three
queries per variant are descriptive observations; p95/p99 are not estimated.

RSS is wait4 lifetime maximum for the direct drain child and OS-accounted reaped
descendants, **not simultaneous whole-stack peak**. The provider runs in the Python
runner, whose separate RSS is unavailable; worker stacks are in drain RSS. CPU
uses the same wait4 accounting scope. Memory is bounded in number of jobs, not
claimed to fit a fixed byte budget for arbitrary documents. Do not sum process
RSS maxima or extrapolate these tiny vectors to a 100k corpus/model workload.

## Invariant evidence

| Contract | Executable evidence | Outcome |
|---|---|---|
| Exclude active/shared hashes, reach later work, pinned identity | `storage_embedding_work`; `parallel_drain` with max_retries=3 | PASS; compiling exclusion/dedup mutations rejected |
| Four active requests, no extra pending capacity | real HTTP readiness + `completed_results_hold_capacity_until_coordinator_handling_finishes` | PASS; overfull admission mutation rejected |
| Publish/refill while another response is held | `parallel_drain` completion-order case, actual DB observations | PASS; exact literal vector and fifth-request readiness |
| Reserve before HTTP, durable restart budget | `parallel_drain_process`; cancellation suites | PASS; rolled-back reservation mutation rejected |
| Late edited/deleted text cannot represent new text | process/thin-slice tests, new/old hash observation | PASS; no request-arrival order assumed |
| Config reset/dimension races | process tests with winning-identity canary and exact key counts | PASS; identity/dimension bypass mutations rejected |
| Empty selection does not discard outstanding work | supervised `tests/parallel.rs` scenario | PASS; premature-exit mutation rejected |
| Single owner; termination releases lock | second real drain, kill/restart and cancellation cases | PASS; ownership bypass mutation rejected |
| Cleanup joins before lock release; panics remain panics | supervised storage-error/worker-panic/error-plus-panic children | PASS; missing-join/lost-panic/swallowed-panic/wrong-hash mutations rejected |
| A child test really ran; no zero-match false success | exact libtest name and one-pass assertion; owned watchdog | PASS; nonexistent-test mutation rejected |
| Measurement rejects wrong contents/count/cap/failure accounting/interruption | 9 Python tests; 2 native observer tests | PASS; 7 Task-4 plus 1 watchdog negative controls rejected and exact bytes restored |

Task-3 controls are in `task3-mutations/receipts.json` (8) and
`task3-extra-controls/receipts.json` (2). Task-4 controls are in
`task4-controls/receipts.json` (7). Earlier cap/dedup and storage-exclusion controls
have separate logs. `task5-watchdog-control-2/receipt.json` additionally rejects a
missing setup timer. Its first version asserted inside the caught boundary and
survived the mutation; the strengthened test asserts the observed deadline outside
that boundary. The final runner installs the stage timer before pin/setup/build
work and retains an incomplete summary on setup cancellation. Failed first-generation oracles were strengthened before
counting those mutations as evidence.

## Actual-tree verification

Host and Linux runner results are recorded below. They cover
current source and locked dependencies; historical standalone proof harnesses
with their own manifests are not implied rerun by a root pass.

- Full performance-tooling suite: `python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py' -q`: **90 tests, 8 optional skips, PASS** (`task5-python-suite-final.log`).
- Native observation oracle: `cargo test --locked --offline --test drain_measurement`: **2 passed, 1 explicit fixture-only ignored**, restored GREEN after its negative control (`task4-native-restored.log`). Campaign cases explicitly execute the ignored observation with a prepared fixture.
- Host storage acceptance: `/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/issue-49/final-host`: **PASS**, all 16 captured commands exit 0. Includes `make verify V=1`: **1,033 passed / 12 ignored in debug and independently in release**, across 75 executable targets each, plus strict Rustdoc. The explicitly ignored real 60-second provider deadline was then selected and passed; schema parity, runner tests, native FTS5/vec1, copied standalone executable and linkage checks passed. No dynamic SQLite/vec1 dependency was found.
- Linux storage acceptance: **PASS** (`final-linux/manifest.json`), all captured commands exit 0, including full `make verify`, explicit real deadline, strict Rustdoc, native capability and copied-executable/linkage checks. Aarch64 Linux / glibc 2.36 / Rust 1.95.0, prepared image `sha256:05ad58ffe40619db2e8231a145571fc0f843d3962072caa26da80ba43f0f07d7`, network disabled. The runner's `--platform host` entry point ran **inside Linux**, not as a claim that the outer `--platform linux` Docker-build wrapper was executed. Only the three locked registry additions were copied into the prepared offline cache. The first attempt is preserved in `linux-initial-failed`: linking ran out of disk before tests. Retry used `CARGO_BUILD_JOBS=2` and `CARGO_PROFILE_{DEV,TEST,RELEASE}_STRIP=symbols`, retaining assertions and stripping symbol tables; these are verification build flags, not benchmark inputs. Native/host captures have identical hashes for all **466** recorded inputs (`linux-parity.json`).

The two Python files changed after native-run input capture only to cover setup
watchdog/cancellation and strengthen its oracle. They passed the final 90-test
Python suite separately; Rust, Cargo, build, vendor, native/schema and Rust test
inputs remain identical. `final-tooling-inputs.json` records that exact delta; it
is not hidden as a purported full-tree hash match. Campaign-4 archives the final
measurement runner; the later oracle-only assertion strengthening does not alter
measurements. All frozen C product input hashes still match the final checkout.

## Architectural review

The algorithm lives in application service `embedding/drain.rs`; no infrastructure
entered domain entities. Storage implements its selection interface using bound
SeaQuery values; SQLite handles never cross threads. Only provider sharing adds
Arc/dynamic dispatch, replacing its former injection generic; existing scoped
storage generics remain. `lib.rs` constructs the real provider and service; CLI
handlers retain their existing responsibilities. No worker process, async runtime,
query batching, schema/config knob, or second owner was introduced. The named
constant is above types/implementations. Private test fixtures are shared only by
their actual consumers. Tests keep Given/When/Then and independent literal oracles;
touched unit/query cases were split into cohesive files rather than growing the
parent test module. Final independent review is pending, separately from tests.

## Remaining acceptance

- Real local-model/provider throughput, actual model-side serialization, rate limits,
  retries under real quotas, and whether concurrency helps that provider.
- Simultaneous whole-stack RSS including the model, 4 GB minimum hardware, large
  vectors/texts/corpus and NFR/gate acceptance. Synthetic RSS is not their substitute.
- x86_64, physical power loss and historical standalone harnesses not named above.
- No merge/push/PR/issue closure is performed by this implementation task. Issue #49
  stays open until its remaining measurement acceptance is explicitly resolved.
