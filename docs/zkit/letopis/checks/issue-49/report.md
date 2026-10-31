# Issue #49 — bounded parallel drain

2026-10-31. Final product includes the HTTP/1.0 correction at `ccb1d6d`.
**Real-provider and whole-stack acceptance remain open.**

[Issue #49](https://github.com/zaurkadiev/letopis/issues/49) ·
[specification](../../../../superpowers/specs/2026-10-31-issue-49-parallel-drain-design.md) ·
[plan](../../../../superpowers/plans/2026-10-31-issue-49-parallel-drain.md) ·
[D-107](../../decisions.md#D-107) · [measurement instructions](../../../../../tests/performance/README.md#bounded-parallel-drain-comparison-49).

## Implementation and review correction

`DRAIN_WORKERS = 4` is above the types in `src/service/embedding/drain.rs`.
One coordinator owns `embed.lock`, file/Markdown work and SQLite writes. Only
provider work crosses to four `threadpool =1.8.1` OS threads through a shared
`Arc<dyn EmbeddingProvider + Send + Sync>`. The sole new direct dependency is
threadpool; locked transitives add num_cpus 1.17.0 and hermit-abi 0.5.3.

Four slots include running requests **and completed responses not yet handled**.
Work selection excludes active hashes inside SQL, before `LIMIT 1`. The coordinator
reserves attempts durably, then sends one text per HTTP call outside transactions.
It publishes whichever result arrives first in one short transaction, frees that
slot only after handling, and selects replacement work immediately. No all-four
barrier, persisted queue, batching, whole-corpus text/vector load or async runtime.
Config/dimension/hash guards remain; empty selection is final only without pending
jobs. Normal/error/unwind cleanup joins every admitted job before releasing the
lock; programming panics remain panics. Forced process death uses existing OS/WAL
cleanup and durable attempts. Existing retry delay may pause the coordinator.

The independent read-only whole-branch review (`b7f627d..14df46e`, gpt-6-astra/high)
found **one Important issue**, no Critical/Minor issues. Campaign-2 had real
HTTP/1.0 implicit-close reuse failures under parallel load: some C cases published
62–63 of 64 vectors while A/B completed 64. A shared-ureq diagnostic probe recorded
245/256 successful requests. Adding a close header only to the fixture did not fix
production behavior. HTTP/1.0 persistence requires negotiated keep-alive; an absent
close header does not permit reuse. [RFC 9112 §9.3](https://www.rfc-editor.org/rfc/rfc9112.html#section-9.3).

The single fix pass keeps pinned ureq and explicitly sends `Connection: close` on
every provider POST, via the imported header identifier and named close value.
There is no extra retry, runtime, dependency, payload, auth, TLS, deadline, schema
or configuration change. This is an implementation ruling, not an earlier user
selection: **every call now establishes a new TCP/TLS connection**, which can hurt
real-provider performance. The final measurement below includes that policy.

`parallel_drain_http10` uses valid Content-Length HTTP/1.0 responses without a
server close header; a gate holds completed response sockets open. Before the fix
only four of eight requests arrive (RED), so the test kills/reaps only its child.
After the fix all eight literal vectors are stored with zero retained failures
(GREEN). Replacing the client header with keep-alive compiles and reproduces the
four-versus-eight failure; restored bytes pass. See `final-http10-red-2.log`,
`final-http10-close-control-2/receipt.json` and `final-http10-close-final.log`.
No second review was commissioned: RED→GREEN, the negative control and final full
platform suites verify the one review finding, as required by Native execution.

## Final measurement provenance

All raw evidence is local and ignored under `.artifacts/issue-49/`. Preserve it to
inspect receipts; no historical evidence was rewritten.

| Variant | Exact product commit | Frozen receipt | Executable SHA-256 |
|---|---|---|---|
| A | `b7f627d1d63d2d7b72989b733838a55925947c17` | `builds/before/build.json` | `010036d3498a365768ee3c6c1aeac0d5c4ab201d58fa817012279acf6d4a6791` |
| B | `8fdab91be6c41c0775f5fcd22905ac9911a560d4` | `builds-final/one-worker/build.json` | `19966884db1b8e81ad750549e0e8911af2ad98c49b21cdc179159a619a8b758d` |
| C | `ccb1d6dcac29b77708f1241b9941ef333b9a05ce` | `builds-final/after/build.json` | `20429d0a7131ef51723c70ab090ffb336cfc0db03603f20dd2a7f7703e7bfbb7` |

A is unchanged sequential main. B is the corrected product with **only** the
worker constant changed to one in a disposable evaluation checkout; C is unchanged
four-worker corrected product. The B patch and commits are in
`builds-final/one-worker.patch` and `builds-final/sources.json`. B used an independent
APFS-cloned release cache; each product package was explicitly cleaned/rebuilt.
There are no target symlinks/hard links. Original receipts, read-only source
archives, binaries/symbols and toolchain hashes remain pinned and verified.
Generated evaluation target caches were reclaimed after freezing; immutable pins
and executable/source archives remain intact.

An earlier shared-target build had reused the B package as provisional C; it was
rejected before any timing, preserved in `builds/correction.json`, and replaced.
Campaigns-1/2 preserve setup/transport failures. Campaigns-3/4 measured the product
**before** the review fix and are historical, not final-product evidence.
`campaign-final` was interrupted by the tool session, retained without a success
claim, and audited for exact owned-process cleanup. `campaign-final-2` is its fresh
completed rerun; it was not selected for a favorable speedup.

The final campaign has **21/21 valid cases**: one conditioning plus five measured
fresh projections per variant in interleaved A/B/C order, then one three-query
overlap schedule per variant. There are 68 knowledge nodes sharing 64 distinct
synthetic texts. Independently supplied literal three-component vectors are checked
through the native observer, outside timing. Each case must publish all 64 unique
vectors, cover all 68 nodes, make exactly 64 document requests and retain no failed
hash/query error. Fixtures/config/input bytes match across all cases.

The owned concurrent loopback provider delays ordinary responses by 25 ms and
query-overlap responses by 100 ms. It explicitly closes connections for all three
variants, so A/B/C all incur connection setup; HTTP/1.0 compatibility without that
header is checked separately by the gated regression. Build/setup/observation are
outside drain time. Deadline: 120 seconds per drain and 20 minutes for the campaign
including setup, with cancellation receipts and exact owned-process cleanup.
Host: Apple M4 Pro, macOS 26.6.2 ARM64; Rust 1.95.0. No external inference or paid
session. Page cache and unrelated host load are uncontrolled. Manifest records
host, every fixture/config/harness hash and native observer source/executable.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/drain_compare.py \
  --before "$PWD/.artifacts/issue-49/builds/before/build.json" \
  --one-worker "$PWD/.artifacts/issue-49/builds-final/one-worker/build.json" \
  --after "$PWD/.artifacts/issue-49/builds-final/after/build.json" \
  --evidence "$PWD/.artifacts/issue-49/campaign-final-2"
```

Use an absent evidence directory to rerun; existing evidence is never overwritten.

## Results after the HTTP/1.0 correction

| Variant | Valid measured calls | Unique vectors / nodes per call | Median seconds (range) | Vectors/s | Max document calls | HTTP attempts / errors per call | Drain RSS median / max MiB | Median CPU seconds | Overlap query median ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A — sequential base | 5/5 | 64 / 68 | 2.5644 (2.5004–2.5887) | 25.01 | 1 | 64 / 0 | 12.64 / 12.81 | 0.1780 | 129.08 |
| B — corrected scheduler, 1 worker | 5/5 | 64 / 68 | 2.5674 (2.4872–2.5837) | 25.19 | 1 | 64 / 0 | 12.58 / 12.61 | 0.1863 | 133.16 |
| C — corrected scheduler, 4 workers | 5/5 | 64 / 68 | 0.6849 (0.6831–0.7132) | 92.15 | 4 | 64 / 0 | 14.30 / 14.44 | 0.1220 | 135.05 |

C/A median speedup: **3.74×**; aggregate throughput ratio: **3.68×**.
B/A median ratio is 1.0012, separating
scheduler overhead from parallelism. Each variant produces 320 verified unique
vectors over five measured cases; all case times remain in the denominator and
no requests/retries/failures are omitted. These are descriptive synthetic results,
not a real-model forecast or a drain SLA. Five samples do not estimate p95/p99.

All nine overlap queries return the literal canary ID; all three per variant
occur during document inference. Maximum total HTTP concurrency is A/B/C=2/2/5,
while document concurrency stays 1/1/4. Queries do not spend drain slots. Three
queries per variant do not establish tail-latency acceptance.

RSS is the drain process lifetime maximum, through wait4 direct-child plus
OS-accounted reaped-descendant accounting; CPU has the same accounting scope.
It includes worker stacks, but **is not simultaneous whole-stack RAM**. The
provider shares the Python runner, whose separate RSS is unavailable. Do not sum
RSS maxima or extrapolate tiny vectors to a 100k/model workload. Slots bound job
count, not a fixed byte budget for arbitrary files or responses.

## Invariants and negative controls

| Contract | Actual executable coverage | Result |
|---|---|---|
| Shared/active hashes cannot hide later work; pinned identity | `storage_embedding_work`, `parallel_drain` with retry budget allowing duplicates | PASS; exclusion/dedup mutations rejected |
| Four submitted/unhandled jobs, including ready responses | real held HTTP, durable reservations and direct completed-but-unhandled capacity test | PASS; overfull admission mutation rejected |
| Publish/refill before a held peer completes | exact DB vector and fifth-request readiness in `parallel_drain` | PASS |
| Attempts reserve before HTTP and survive death | `parallel_drain_process`, cancellation suites | PASS; rollback mutation rejected |
| Late edit/delete cannot represent a new text | process/thin-slice new/old hash assertions, unordered exact request set | PASS |
| Config/dimension races preserve current state | winning-identity canary and key counts; native dimension guards | PASS; identity/dimension bypass rejected |
| Empty selection waits for outstanding work | supervised embedding unit child | PASS; premature completion rejected |
| Single drain owner, restart and lock release | second real process, kill/restart/cancellation | PASS; ownership bypass rejected |
| Join before lock release; panic stays panic | supervised error/panic/error-plus-panic cases and watchdog | PASS; missing joins, lost/swallowed panic and wrong hash rejected |
| Libtest child actually ran | exact case name and one-pass check | PASS; zero-match mutation rejected |
| Correct vectors/counts/cap/accounting/cleanup/watchdog | 9 Python runner tests and native literal-vector observer | PASS; 7 Task-4 controls plus setup-timer omission rejected |
| Valid HTTP/1.0 without server close header | gated `parallel_drain_http10` and keep-alive mutation | PASS after correction; 4/8 before, 8/8 after |

Receipts: `task3-mutations/receipts.json` (8),
`task3-extra-controls/receipts.json` (2), `task4-controls/receipts.json` (7),
`task5-watchdog-control-2/receipt.json`, `final-http10-close-control-2/receipt.json`.
Earlier capacity/dedup/exclusion controls have separate logs. Initial oracles that
survived adverse mutations were strengthened before counting the final rejection.
All mutated source bytes were restored. An extra close+keep-alive header experiment
retained close and was not an adverse mutation; the final control replaces its value.

## Final-tree platform verification

Final platform receipts below are complete and verified against the current tree.
Historical `final-host`/`final-linux` are pre-fix evidence; they are not reused.

- macOS corrected tree: **PASS**, `final-host-fixed/manifest.json`. Command: `CARGO_INCREMENTAL=0 /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/issue-49/final-host-fixed`. Its `make verify V=1` ran formatting, all-target check/Clippy, strict Rustdoc and root suites: **1,034 passed / 12 ignored in each of debug and release**, 76 executable targets per profile. All captured commands exited 0.
- Linux corrected tree: **PASS**, `final-linux-fixed/manifest.json`. Command inside the owned container: `python3 -B tests/storage/run.py --platform host --evidence /receipt-issue49-fixed`. Full `make verify` passed: **1,028 passed / 12 ignored in each of debug and release**, 76 executable targets per profile. All captured commands exited 0.
- Performance tooling: 90 tests, 8 optional skips, PASS in `task5-python-suite-final.log`; these Python sources are unchanged by the HTTP fix.

Both suites contain the gated HTTP/1.0 regression. Their explicitly selected
ignored real 60-second provider deadline also passed (60.00 s macOS / 62.71 s
Linux wall time under load). Schema parity, runner tests, strict Rustdoc, native
FTS5/vec1, copied standalone executable/reopen and linkage checks passed; no dynamic
SQLite/vec1 library dependency was found. Twelve default ignored cases per root
profile are disclosed, not counted as passed; the selected deadline is additional.

`final-receipts.json` and `final-receipts-check.log` verify **467/467 identical
input hashes across macOS, Linux and the final current tree**, complete command
exit codes, both regression results, exact frozen-product parity, all 21 final
campaign cases, and the compiled keep-alive negative control. No post-capture
Python/source delta remains in this final verification. The final nine-test
measurement oracle rerun passed as part of that gate. All three frozen source and
executable manifests were reverified; measured C exactly matches final product
inputs at `ccb1d6dcac29b77708f1241b9941ef333b9a05ce`.

The Linux runner uses its `--platform host` entry point **inside** a prepared
network-isolated Linux ARM64 container, not the outer `--platform linux` image-build
wrapper. Pinned image: `sha256:05ad58ffe40619db2e8231a145571fc0f843d3962072caa26da80ba43f0f07d7`;
glibc 2.36, Rust 1.95.0. Only three new locked registry packages were copied into
its offline cache. It uses `CARGO_BUILD_JOBS=2`, stripped symbol tables and the
image's zero-debug/incremental settings to fit disk. macOS sets `CARGO_INCREMENTAL=0`
to avoid cache growth. These verification flags do not alter benchmark builds.
The earlier Linux disk-exhaustion receipt is retained; only task-owned generated
incremental/evaluation caches and owned containers were reclaimed. No unrelated
source, image, volume or container was pruned.

Root suites include all plan targets and the new HTTP/1.0 target. Historical
standalone manifests not named in the receipts were not implicitly rerun. Physical
modules, visibility, trait implementation/import direction and `lib.rs` wiring
were checked separately: pure domain, service-owned orchestration, SQLite staying
on the coordinator, bound SeaQuery values and actual composition are preserved.
No wrapper-only service, speculative interface, new generic propagation or async
layer was introduced. New/touched code keeps focused verb-named functions and
Given/When/Then tests. Independent review found no additional minor issues.

## Remaining acceptance

Real-model/provider throughput (including any TLS/connection-setup cost), model-side
serialization, provider quotas/retries, simultaneous whole-stack RSS, minimum 4 GB
hardware, large vectors/texts/corpus and NFR/gates remain **not assessed**. Also
unassessed: x86_64 and physical power loss. No paid/model sessions were authorized.
Issue #49 remains open for that acceptance; no merge/push/PR/closure is performed.
