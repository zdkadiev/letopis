# Native Samply and check/status optimization

2026-11-06. Genuine Samply recordings confirmed repeated Git tree structural
validation as a CPU cost. The focused optimization is implemented and verified:
**check median 42.452 → 32.038 s (24.53% lower), status 42.218 → 31.566 s
(25.23% lower)** on the populated synthetic 100k corpus.

The real local Ollama population and full-coverage hybrid measurements also
completed. The earlier permission failures remain historical evidence; they do
not describe this unrestricted executor.

## Ordinary comparison

| Command | Samples per release | Before median, s | After median, s | Latency reduction | Speedup | Maximum observed CLI RSS, MiB, before → after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| check | 3 | 42.451972 | 32.037880 | 24.53% | 1.325× | 377.88 → 375.19 |
| status | 3 | 42.218257 | 31.566077 | 25.23% | 1.337× | 376.75 → 372.47 |

All twelve fresh-process calls passed independent CLI and projection-count
oracles: 100k knowledge nodes, 100k source nodes, 500k edges, empty check findings,
clean status problems/locks. Inputs contain 3334 Markdown files and 1000 source
files. All 4334 canonical file hashes were checked. Embeddings were disabled in
this comparison fixture; the concurrent Ollama experiment uses a separate copy.

Three pairs alternate before/after, after/before, before/after for each command.
The already initialized and captured fixture follows prior successful recordings;
there are no extra conditioning calls or controlled cold page-cache claims.
Preparation and correctness checks are outside the CLI timer. The ordinary
campaign began after root verification and all profiling had ended; no owned
compilation/tests were started during it. A real sequential Ollama drain continued
on the shared host throughout both sides. This is not a global process audit or
an isolated-hardware result. n=3 does not support tail quantiles or a formal SLO.

RSS is wait4 lifetime high-water memory for the direct child and OS-accounted
reaped descendants, not simultaneous whole-stack/model memory. Status median
RSS slightly increased, 371.6875 → 372.34375 MiB; no memory improvement is claimed.
Raw CPU, RSS and OS block-operation counters remain in the samples; OS blocks are
not I/O bytes.

[Samples](comparison-samples.jsonl), [summary](comparison-summary.json),
[manifest](comparison-manifest.json), [exact campaign driver](compare.py),
[independent data review](data-review.md).

## Build and host identity

Before is the earlier loose-object-reuse release, captured local clone commit
`3b2e477cd2bb8bd204ab606a1566f32e713b4ac8`, binary SHA-256
`77f8a66d0b0edf76799cb562537839d78fca20a2e78784c7dfe434ec9a8c7e8f`.
After is measurement-only clone commit `3d820de41ee12385bc734ac4ceeb56b78ba379f9`,
binary SHA-256 `9a3e2b70a907d135604ad04e6491e358d07575e987c1457d514a1820c2d2ba6a`.
The current root matches all 106 captured after-release product input hashes.
The before/after product inputs differ only in objects.rs, reader.rs and the
validation.rs test counter; loose-object reuse is present on both sides.

Both releases use Rust 1.95.0 and locked offline dependencies, with release debug
symbols (`CARGO_PROFILE_RELEASE_DEBUG=1`). Native profiles retain presymbolicated
sidecars, so later Cargo outputs are not their symbol identity. Main remains at
`52ed491` with reviewable uncommitted changes; measurement commits exist only in
isolated local clones. No push, merge or issue publication was performed.

Current read-only host inventory: Apple M4 Pro, 14 logical CPUs, 48 GiB RAM,
macOS 26.6.2 arm64, APPLE SSD AP0512Z (500277792768 bytes). This is not the project's
minimum-hardware or production acceptance environment.
[Before inputs](before-build.json), [after inputs](after-build.json),
[host inventory](host.json). The previously measured 5.14× inspect improvement
belongs to the [earlier comparison](../2026-11-06-100k/report.md); it was not rerun
or newly attributed to this memo.

## Genuine Samply recordings

Samply 0.13.1 now successfully launches both commands. Its separately requested
reinstallation also succeeded with `cargo install samply --version 0.13.1
--locked --force`; the installed executable hash and log are retained. Recording
already worked before reinstall in the new executor, so reinstall is not claimed
as the cause of resolving the old Mach-bootstrap restriction.

| Command | Before usable stacks | After usable stacks | Before observed CPU share in structural validation | After observed structural-validation samples |
| --- | ---: | ---: | ---: | ---: |
| check | 41436 | 36423 | 23.76% | 0 |
| status | 38915 | 37262 | 25.10% | 0 |

Zero observed samples mean no samples
landed in that function, not zero validation calls. First-use validation and full
closure audits still execute. Historical hashing accounts for approximately 66%
of observed CPU after the change, including raw object decoding and identity
verification; live source hashing accounts for about 28–29%. These inclusive
scopes overlap and cannot be added.

Profiles use save-only 1000-Hz recording and local presymbolication; no upload,
browser or sampling server is started. Before and after profiles were recorded
under different shared-host activity. Their sampled CPU totals are not the
ordinary latency comparison above. The first original launcher recorded both
commands successfully but its final validator selected an old default Python;
independent Python 3.12 validation accepted both outputs, stack streams and graph
counts. The [new launcher](run-samply.zsh) selects Python 3.12 explicitly and
completed end to end after reinstall.

[Native summary](native-summary.json), [before receipts](before/validation.json),
[after receipts](after/validation.json), [install log](samply-install.log),
[installed version](samply-after-install-version.txt),
[installed hash](samply-after-install.sha256). Each before/after directory contains
both raw compressed profiles, symbol sidecars, result JSON and attributed stacks.
To record again from this checkout, pass a fresh absolute output directory to
`./docs/zkit/letopis/checks/issue-65/native-followup/run-samply.zsh`.

## Change and guarantees

`ObjectReader` retains at most 4096 successfully validated tree OIDs for its
invocation-owned lifetime. Each tree read still reads and decodes the current
object, checks its exact type and independently verifies its content identity
before consulting that metadata. It retains no tree/blob payload cache. Invalid
or unavailable objects are not memoized; overflow validates normally without
rejecting valid inputs. Separate readers start empty; existing clones share their
command-owned reader.

Full closure audits still call structural validation directly on every visited
tree for every audit. Current source captures, observation demand/error ordering,
strong pins, ref durability, storage, services, public interfaces and composition
are unchanged. The larger historical-file-payload cache was deferred because it
could conceal corruption between different addressed reads. No dependency,
runtime, Git subprocess, filter or network fallback was added.

[Specification](../../../../../superpowers/specs/2026-11-05-issue-65-refresh-design.md),
[execution ledger](../../../../../superpowers/plans/2026-11-05-issue-65-refresh.md),
[independent code/contract review](code-review.md).

## Verification

- `make verify V=1` passed on the actual changed root: formatting, all-target check,
  Clippy, fixed-value lint, all root debug/release tests and strict Rustdoc. Each
  library profile reports 601 passed / 9 ignored. The complete log has 2405
  successful test executions and 22 ignored executions including repetitions and
  examples; this is not a unique-test count. [Log](final-verify.log),
  [receipt](verification.json). Earlier permission failures are not reclassified;
  this is a fresh successful host run.
- Reuse regression failed before implementation (2 structural validations versus
  expected 1). All 57 Git tests subsequently passed. Five new tests cover both
  SHA formats, reader isolation, corrupt/wrong-type/missing replacements,
  restoration, invalid structures, full audits and metadata-budget overflow.
  [RED](red.log), [Git suite](git-tests.log).
- All five hand mutants were rejected: disabled reuse, ignored content identity,
  cached invalid structure, exceeded metadata limit and omitted full-audit
  validation. Mutations used a separate source and Cargo target. After restoration
  that target was explicitly rebuilt and all 57 Git tests passed again.
  [Mutants](mutations.json), [exact mutator](mutate.py),
  [restored target](restored-git-tests.log).
- The first expanded Git run exposed a fixture restoration error: the test tried
  to overwrite a read-only loose object. The fixture now unlinks its own object
  before restoring it; the subsequent full Git suite is green. The initial
  [fixture failure](green.log) is retained, not described as a product failure.
- Python: 84 tests, 76 passed and 8 explicit integration skips in the clean pinned
  measurement clone. A dirty-root attempt had two build-pin precondition errors
  in `test_rejects_changed_product_source_before_measurement` and
  `test_rejects_symlink_and_modified_binary`; the runner intentionally refuses
  dirty product pins. [Pinned run](python-pinned-tests.log),
  [dirty-root attempt](python-tests.log).
- Independent code and data reviews confirmed the contracts, hashes, arithmetic
  and result oracles. The [full-data review](full-data-review.md) also verified
  native population and full-query receipts. Its reporting precision point was
  resolved by separating output-membership evidence from native path-execution
  evidence. No product code change was required. No unrelated historical proof
  harness, Linux/x86_64 runner or production pilot was rerun. Product
  storage/native inputs are unchanged.

## Complete 100k Ollama population and real hybrid search

The real local `embeddinggemma:300m-qat-q4_0` run completed with **100000 native
768-dimensional vectors for 100000 knowledge entries**. Independent read-only
SQLite/vec1 verification found no missing vector/key mappings, wrong vector
widths, orphan hashes, knowledge entries without vectors, recorded provider errors
or leftover attempt reservations. The same configuration identity was retained.

Population used two segments, not one uninterrupted latency sample:

| Segment | Outcome | Vectors at end | Measured CLI interval, s |
| --- | --- | ---: | ---: |
| Initial | Controlled censoring, exit -9 | 51024 | 3268.658 |
| Same-config continuation | Successful command completion | 100000 | 5245.203 |

The first segment was stopped before its 3600-second budget at a verified
zero-reservation checkpoint. SIGSTOP/SIGCONT observations and the termination
receipt are retained. The controller wrote no database data, refunded no attempt
and changed no configuration or retry setting. The initial runner's assertion
failure is expected censoring, not a successful complete drain or provider failure.
The continuation started with all 51024 vectors intact and zero attempt rows.

Total observed population interval including the 5.810-second restart gap:
**8519.671 s (141.99 min)**, about 11.738 unique
vectors/s. Active CLI intervals sum to 8513.861 s; their wait4 CPU totals sum
to 2925.097 s. These CLI counters exclude Ollama daemon/model CPU, GPU and memory.
The initial segment overlapped verification, profiling and ordinary comparisons;
the host was shared throughout. This is one split population experiment, with
uncontrolled model/OS caches, not a steady-state or production throughput verdict.

[Native coverage](ollama/native-complete-coverage.json),
[first segment](ollama/drain.json), [checkpoint](ollama/controlled-checkpoint.json),
[continuation](ollama/resume-drain.json), [identity](ollama/complete-coverage.json),
[coverage SQL](coverage.sql), [oracle build](native-oracle-build.json).
The independent coverage scan precedes query timing and reads all vectors; no
query is described as cold.

Three ordinary full-coverage mixed searches used the **new pinned release** and
`case 000000`, limit 5. An independent FTS query finds exactly one literal URI;
each product result retains that URI and adds four other known URIs with clean
flags. This confirms vector contribution and retention of the independently
expected lexical hit, ruling out pure FTS fallback. Output membership alone does
not prove that the lexical branch caused the retained hit; execution of both
retrieval paths is established separately by the native recording below. No
semantic relevance-quality verdict is inferred.

| Measurement | Result |
| --- | --- |
| Full hybrid CLI samples, s | 111.853, 111.938, 111.007 |
| Median, s; n=3 | 111.853 |
| Median CLI CPU, s | 106.382 |
| Maximum observed CLI RSS, MiB | 187.84 |
| Enabled-model check, one valid call, s | 32.129 |
| Enabled-model status, one valid call, s | 33.396 |

The full check/status calls are separate observations, not extra samples in the
paired model-free comparison. Full status reports zero backlog, failed embeddings,
problems and active locks. No owned drain/tests/compilation ran during these full
ordinary reads. n=3 supports no p95/p99 claim. Model resources remain outside CLI
wait4 RSS, and no minimum-hardware, relevance or formal NFR gate is closed.

[Full query summary](ollama/full-hybrid-summary.json),
[full read manifest/oracles](ollama/full-reads-manifest.json),
[full check](ollama/full-check.json), [full status](ollama/full-status.json).
A separate genuine Samply mixed-query recording was validated after ordinary
measurements. Its observed CPU shares are eligibility 96.91%,
Markdown parsing 53.64%, historical hashing
20.02%, live source hashing 8.40%
and native vector retrieval 1.90%. These inclusive
scopes overlap and cannot be added. The engine requests K equal to stored vector
count; application eligibility and repeated parsing remain the main search cost.
This was diagnosed and drafted for follow-up, not changed in this implementation.
The separate native recording contains 23 observations of
`text_index_get_by_expression`, 20 of `fts5Bm25Function` and 2340 of
`vector_index_get_by_vector`, demonstrating execution of both retrieval paths
for that profiled request. [Channel evidence](ollama/full-channel-evidence.json).
[Raw profile](ollama/full-native-search.json.gz),
[symbols](ollama/full-native-search.json.syms.json),
[stacks](ollama/full-native-stacks.json), [validation](ollama/full-native-validation.json).

Earlier partial-coverage vector-only probes are retained separately: coverage
20523–23032, first copied-fixture refresh/query 89.680 s, repeats 30.766 and
31.679 s, independent zero-FTS and known-URI oracles. Their pre-memo release,
coverage, concurrency and query differ from the complete-coverage results;
they are not pooled or used as a search before/after speedup.

A separate three-second macOS `sample` window during resumed drain produced
2569 main-thread observations: 1345 in HTTP input waiting (52.36%), 954 in next-work
SQL (37.14%). These are wall-stack observation shares, not a Samply CPU profile
or exact whole-run time fractions. They confirm queue selection as another cost.
[Wall stacks](ollama/resume-wall-sample.txt), [summary](ollama/resume-wall-summary.json).

An embed_failures row with error=null is an attempt reservation, not a provider
error. Early raw progress receipts named that table-row count `failures`; interpret
it as attempt rows. Final reserved and recorded-error counts are both zero.
The API's one-time 302304460-byte resident model allocation observation is not peak
model RSS or simultaneous whole-stack RAM. Paid providers, production pilots,
controlled cold caches and Linux/x86_64 were not measured here.

[Follow-up drafts](follow-ups.md) remain local; no issues or formal gates were closed.
