# Issue #65: staged performance diagnosis

Date: 2026-10-30. Status: approved by the user; staged approach selected.
Issue: [#65](https://github.com/zaurkadiev/letopis/issues/65).
Branch: `issue-65`. Initial product baseline: `f7e0c6c90a7e4d4aa49e0a0c788c89ef7317c43b`.

## Intent and boundaries

Find where the current CLI spends time and resources, establish causes, and rank
useful optimization work. Measure before changing algorithms. The deliverable is
reproducible diagnostic tooling, evidence and a report, rather than a faster
implementation selected in advance.

This is model-free work initially. No real provider/model requests, downloads or
paid sessions are authorized. Real hybrid search and model-backed drain remain
explicitly unassessed until separately authorized. A managed loopback fake can
exercise vector storage, query embedding and sequential drain, but its timings
describe client/storage overhead, not actual inference or production hybrid speed.

No formal gate, TK-35 latency verdict, TK-34 memory verdict, or usefulness claim
follows from this work. Do not renumber roadmap tasks or close related issues.
There is no async runtime, daemon, public diagnostic command, permanent tracing
platform, or speculative optimization in this scope.

## Sources and current implementation

- [Program structure and review](../../zkit/letopis/architecture.md#program-structure),
  [decisions](../../zkit/letopis/decisions.md) and
  [mechanics](../../zkit/letopis/mechanics.md) retain authority over runtime,
  synchronization, freshness and snapshot verification.
- [CLI](../../zkit/letopis/contracts/X-1-cli.md),
  [embeddings](../../zkit/letopis/contracts/X-2-embeddings.md),
  [Git](../../zkit/letopis/contracts/X-3-git.md) and
  [read view](../../zkit/letopis/contracts/I-8-read-view.md) define correctness.
- [TK-35](../../zkit/letopis/roadmap/epics/EP-4-system-validation/tasks/TK-35-search-latency.md)
  requires actual populated 100k-vector hybrid search for its main verdict.
- [Read optimization report](../../zkit/letopis/checks/read-workflow-performance/report.md)
  records PR #64: routine reads stopped auditing unrelated snapshot descendants;
  full check/reindex retain full audits. Historical three-sample observations
  identify capture/verification work as a candidate, not a current conclusion.
- `src/commands.rs` wires each invocation; `src/lib.rs` remains the composition
  root. Git subprocesses are owned by `src/git/process.rs`; SQLite and vector
  operations belong to storage; embedding orchestration remains a service.
- `tests/target_corpus/generate.py` already generates deterministic corpora and
  independent input manifests. Its existing model-specific load/drain runner
  must not be invoked blindly: it contacts a real local provider.

## Approaches

1. **Staged diagnosis, selected.** Survey every listed command on 1k/10k, then
   profile the expensive scenarios and extend to 100k. Early results guide
   profiler work; the final report still accounts for the entire issue matrix.
2. Full factorial campaign immediately. More uniform coverage, but expensive
   before knowing which command, scale or wait dominates. Rejected for this task.

The user approved this specification and selected samply on 2026-10-30.
The implementation plan was separately approved before execution.

## Diagnostic tooling

Keep an external Python standard-library runner under `tests/performance/` and
reuse the existing corpus generator without moving or rewriting it. Separate
scenario setup, process measurement and reporting only where they own real
responsibilities. Do not build a general benchmark framework or new service layer.
Exact filenames and profiler adapters are implementation-plan choices.

Measure a locked/offline release build with symbols if needed. Record the full
product commit, source/build-input hashes, toolchain, Cargo settings and binary
SHA-256. Harness revision is recorded separately. A changed source tree or build
gets a new identity and baseline; reject an unverifiable binary/source pairing.
Do not measure a debug build or include compilation in CLI latency.

Work only in generated standalone repositories and new evidence directories.
Use isolated Git configuration, explicit disabled embeddings for FTS scenarios,
and explicit fake-provider configuration for model-free vector scenarios.
Never borrow or modify the user's repository, `.letopis` state, real provider
configuration or existing assessment receipts.

## Stage A: survey

Generate 1k and 10k knowledge-node corpora with fixed seed, 2,000-byte fragments,
30 elements per file, source addresses and links using the existing generator.
Record actual file/node/edge counts, byte sizes, Git objects and vector coverage;
node count is not file count. These synthetic corpora are capacity probes, not
production traces. Additional packing variation is driven by an observed cost.

Survey unchanged `search`, `inspect`, bounded `traverse`, unscoped `check`,
`status`, plus one-file refresh, several-file refresh, bulk refresh and full
`reindex`. Keep queries, requested IDs, traversal limits and edits deterministic.
Record limits; a bounded traversal cannot establish whole-graph performance.
Use an unchanged FTS search as the refresh carrier for edit scenarios. Set up
edits before the timer, and restore equivalent independent fixtures between
mutation samples; the second call after an edit is a different scenario.

For inexpensive repeatable scenarios, take two labeled conditioning calls and
20 measured fresh-process calls. For expensive refresh/reindex and later 100k
runs start with three observations, report individual values/range/median, then
add repetitions only when the question needs them. Do not publish p95 below
100 observations or p99 below 1,000; preserve sample count and quantile method.
These are reporting conventions, not statistical confidence guarantees.

Every measured invocation is a new CLI process. Label first use, initialized
projection and conditioned repeat separately. Only call a cache cold when its
state was controlled and documented; otherwise OS-cache state is uncontrolled.
No privileged cache flushing or interference with unrelated workloads.

## Stage B: explain the expensive cases

Rank observed absolute time, resource use and growth between 1k/10k. Collect
CPU and wall-time stack samples/traces for expensive or unexpectedly scaling
scenarios; inspect waits even when CPU use is low. Re-run each selected case
unprofiled and profiled on equivalent inputs and report profiler overhead.
Do not blend profiled samples into the ordinary latency distribution.

Use **samply** as the primary CPU profiler, with macOS `sample` for supplementary
wall-time stacks where useful. Samply 0.13.1 was installed from the official
ARM64 release with SHA-256 verification. A separate 300-ms smoke program yielded
300 samples outside the Codex sandbox; sandbox execution failed with
`Unknown(1100)`. This establishes tool availability, not Letopis performance.
Use `samply record --save-only` so measurement opens no browser/server and uploads
nothing. Record tool versions, supported metrics and execution permissions.
macOS is the initial execution target. Linux can be added with a supported
adapter; unavailable platforms/tools/metrics remain named gaps. No silent tool
installation or claim that CPU sampling measures blocked-time attribution.

Attribute startup/wiring, Git discovery/capture/verification/retention,
Markdown parsing/hashing, SQLite/FTS/vec1, embedding client and serialization
where evidence permits. Count Git invocations separately from timing baselines.
`GitProcess` deliberately strips `GIT_*` variables; do not assume a trace
environment variable reaches it. A PATH wrapper may record operation/count/time
without reading stdin or logging blob contents; nested wrapper times overlap.

If stacks/external traces cannot resolve a suspected cost, add the smallest
scoped diagnostic counters at the owning module. Identify instrumented builds
separately, preserve default behavior and compare overhead. Do not move
infrastructure into domain/services or widen interfaces solely for profiling.
Unattributed time is reported rather than assigned a fabricated breakdown.

Confirm each bottleneck hypothesis with a bounded one-variable experiment,
such as source-tree size, number of changed files, packing, or concurrent writer
presence. Keep baseline inputs, config and build fixed otherwise. A mutation
that disables verification or returns incomplete results is not evidence of a
correct optimization. Controls support only the causal conclusion they isolate.

## Stage C: scale, vector client and contention

Extend the survey to 100k with bounded repetitions. Stage order reduces wasted
profiling, not acceptance coverage: each issue scenario receives evidence or an
explicit reason for being unassessed. Record missing scale rather than extrapolate.

A deterministic loopback fake provider supplies valid vectors for a distinct
client/storage experiment. Prove current hash/config coverage and vector
participation with controlled fixtures and observations, rather than a successful
search response alone. Sequential drain is included here. Fake-vector ordering
has no semantic relevance claim; real hybrid latency remains unassessed.
Do not use live models even if installed. Parallel drain is assessed only if #49
is implemented in the measured product revision; otherwise mark it unavailable.

Run reads against isolated `reindex` and fake-provider drain writers. Begin with
one reader/one writer, increase only if necessary to explain contention. Capture
the actual overlap, completed/failed/timed-out calls, throughput, latency, SQLite
and filesystem lock waits where observable. Search has service writes as well
as reads; neither assume pure-read behavior nor classify all waiting as SQLite.
Validate snapshot-consistent results and post-run projection integrity.

## Measurements and evidence

Retain a manifest, per-invocation JSONL measurements, summary, profiler artifacts
and report in a new ignored directory. Keep durable findings/reproduction steps
under `docs/zkit/letopis/checks/issue-65/`, with links/hashes to raw evidence.
Never overwrite an earlier run. Evidence includes:

- Hardware/OS/tools, process scope, build identity, corpus/config hashes,
  scenario/request, cache labels, observed load and measurement conditions.
- End-to-end monotonic duration, exit/outcome, CPU, peak RSS and I/O where
  supported, subprocess/Git counts and time, SQL transactions/query counts and
  lock waits, parsed/hashed bytes when directly observable. Mark each missing
  metric unavailable; sampling RSS is not an exact lifetime peak.
- Distinct scopes for CLI, children, fake provider and any future real model.
  Child CPU/I/O and RSS aggregation semantics are explicit; per-process peak
  sums are not simultaneous aggregate peak. Overlapping intervals are not added.
- Raw samples, profiler settings, profiler overhead, correctness observations,
  experiment control/change and evidence references for every ranked claim.

Generated fixtures contain no user secrets. Do not copy ambient credentials,
raw provider payloads or the user's knowledge into receipts. Preserve only
necessary request/result evidence, bounded diagnostics and hashes. Correctness
uses fixed fixture expectations, not output from the implementation under test.

Use per-invocation and per-stage watchdogs, proposed defaults 120 seconds and
20 minutes. A selected 100k/full-audit invocation can have a recorded override
up to 600 seconds. Report timeouts as censored observations, never as successful
latency values. Retain partial evidence and stop a stage at its budget rather
than retry indefinitely. Fixture/build time has its own visible accounting.
On cancellation/timeout reap only owned processes/groups, including an owned
detached drain and fake provider; never kill by process name or remove lock files.

## Verification and completion

Test measurement accounting, invalid inputs, timeouts/cleanup, summaries and
correctness rejection with tiny fixtures. Mutation-check new behavioral tests;
follow Given/When/Then and focused function/file rules from AGENTS.md. Verify
actual release execution on small inputs before collecting expensive samples.
Product Rust changes, if diagnostic hooks prove necessary, require `make check`
and `make verify`; ordinary runner/docs checks do not substitute for those.

The report contains a coverage table for every requested scenario/scale/mode,
reproduction commands and tool/platform limits. Each candidate is labeled
confirmed, suspected, or not reproduced. Rank confirmed findings by measured
impact with code owner, absolute costs, contribution where defensible, experiment
and evidence. If no important bottleneck is confirmed, say so.

Prepare separate optimization issues with baseline, proposed change, improvement
criterion, risks and regression checks. Publishing those issues and updating
GitHub uses the user's applicable authorization; drafts can be reviewed locally.
No optimization implementation is included automatically. The final verdict
states assessed coverage and remaining model/platform/scale gaps; it must not
present an unavailable measurement as passed or close unrelated gates.

Builder check: no important unanswered questions. Local document links checked.
Implementation plan separately approved; Native execution produced model-free
measurements, samply profiles and causal controls. See the [report](../../zkit/letopis/checks/issue-65/report.md)
for assessed coverage, explicit gaps and final review status. Real models and
paid sessions remain unassessed. Specification work itself ran no Letopis benchmarks.
