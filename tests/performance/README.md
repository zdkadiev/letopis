# Model-free performance diagnosis (#65)

The [search optimization experiments](../../docs/zkit/letopis/checks/issue-65/search-optimization/report.md)
compare four methods and record the selected changes: 111.886 → 61.102 s median
on the synthetic 100k corpus with real Ollama vectors. Its
[commands](../../docs/zkit/letopis/checks/issue-65/search-optimization/commands.md)
cover verification, repeat measurements and Samply recording. This real-provider
follow-up is separate from the model-free harness described below.

The [native Samply follow-up](../../docs/zkit/letopis/checks/issue-65/native-followup/report.md)
records the verified check/status optimization, genuine before/after profiles,
and the separately authorized real Ollama campaign. Its coverage status is explicit.

The [2026-11-06 100k/Ollama attempt](../../docs/zkit/letopis/checks/issue-65/2026-11-06-100k/report.md)
records successful 100k initialization with a larger budget, current command
timings, and the real Ollama provider attempt blocked by sandbox loopback access.

The [2026-11-05 follow-up](../../docs/zkit/letopis/checks/issue-65/current/report.md)
measures the current library-backed Git implementation and records the focused
loose-object reuse optimization. Historical reports below retain their captured
revisions. The scenario name `reindex` is kept for existing comparison files;
current surveys and contention writers execute `index --full`.

Use `/opt/homebrew/bin/python3.12 -B` on this host; system Python is too old.
The runner uses new synthetic standalone repositories, explicit disabled/fake
embedding settings and immutable evidence. It never uses the working repository's
`.letopis` configuration or calls a real provider.

`python3 -B tests/performance/run.py --list-scenarios` lists scenarios without
building or running the product.

Build inputs, release executable and harness are pinned separately. Cargo builds,
fixture generation, projection initialization and independent sample resets are
setup, outside end-to-end CLI latency. Each measured call starts a new process.
Copying changes file inodes, so copied projections are refreshed before edits.

Raw output belongs in a new ignored `.artifacts/issue-65/` directory. Never replace
an earlier run or label uncontrolled page-cache state cold. These synthetic probes
and fake-provider overhead do not establish true hybrid latency, model cost,
real production relevance, or any formal project gate.

## Run the campaign

All output paths must be absent. Examples assume execution from the repository
root and use the selected Python 3.12 interpreter. A first run builds an offline
release with debug symbols and stores its immutable executable under the evidence
directory. Later commands use that `build/build.json`, never a mutable Cargo path.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage survey \
  --nodes 1000 --initial-timeout 600 --evidence .artifacts/issue-65/new-survey-1k
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage profile \
  --nodes 1000 --scenarios inspect traverse \
  --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --fixture-manifest .artifacts/issue-65/new-survey-1k/fixture/corpus.json \
  --evidence .artifacts/issue-65/new-profiles-1k
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage survey \
  --nodes 10000 --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --evidence .artifacts/issue-65/new-survey-10k
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage scale \
  --nodes 100000 --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --evidence .artifacts/issue-65/new-scale-100k
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage contention \
  --nodes 60 --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --evidence .artifacts/issue-65/new-contention-60
```

Default call timeout is 120 seconds; a separately recorded initial-publication
override can be at most 600 seconds. A global 20-minute stage watchdog also covers
setup; `--stage-seconds` can shorten it. Failure retains manifest, completed
samples and a completion failure receipt. Each record is flushed before the next
call. Missing initialized projection stops that scale; it does not become a zero
latency or a failed product correctness assertion.

Cheap calls use two conditioning plus 20 measured invocations. Read-only calls
observed above two seconds switch to three measured observations; edit/reindex
and 100k calls start with three. `--repetitions` is an explicit recorded override
for smoke/extended assessment, not a silent claim of default coverage. Each edit
sample starts from equivalent copied inputs and a refreshed pre-edit projection.
No p95 below 100 successes or p99 below 1000; failures/censored attempts remain in
attempt counts and throughput denominators. Quantiles use nearest rank; median
averages the middle pair.

## Profilers and causal controls

Samply uses `--save-only --rate 1000 --unstable-presymbolicate`; it starts no
browser/server and uploads nothing. On this Mac it requires execution outside
the Codex sandbox. `.json.gz`, `.syms.json` and inclusive `stacks.json` remain
separate from ordinary timing series. Three alternating unprofiled/profiled
pairs quantify observed wrapper overhead; the host itself is not isolated.
Supplementary macOS `sample` attaches only to an exact owned PID. System Git is
not represented as a sampled process in retained samply captures; no CPU totals
are invented for it.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/controls.py \
  --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --nodes 60 --variable external --evidence .artifacts/issue-65/new-source-control
/opt/homebrew/bin/python3.12 -B tests/performance/controls.py \
  --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --nodes 60 --variable packing --evidence .artifacts/issue-65/new-packing-control
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --stage profile \
  --nodes 100000 --profile-window 30 \
  --baseline .artifacts/issue-65/new-survey-1k/build/build.json \
  --evidence .artifacts/issue-65/new-censored-window
```

The external control preserves fragment lengths and author source tree but removes
source declarations. It is diagnostic work on a different workload, never a valid
optimization. Packing varies 30 → 10 elements/file; source tree, counts and first
addressed bytes remain fixed, while derived path IDs/includes boundaries can vary.
Git counters use a separate transparent PATH wrapper and retain only operation,
PID, exit and timing. Their Python startup overhead excludes those runs from
ordinary latency distributions. Do not sum overlapping intervals.

A censored profile window deliberately stops its own CLI after at most 120 seconds
and lets a root wrapper finish so samply can save. Wrapper success is explicitly
`profile_window_only`; `child.json` remains timeout/nonzero/success independently.
It is never included in ordinary successful latency. `letopis-stacks.json` filters
the profile to the actual CLI PID rather than mixing Python CPU into its frames.

Cancellation propagates after persisting the interrupted attempt; it stops later
scenarios/pairs. Outer TERM/INT also cancels nested window workloads. Cleanup
allows bounded grace for the entire process group, including when its leader
already exited. Wall sampling of a refresh uses a separate pre-edit fixture.
The inspect oracle freezes the expected anchor/range/hash before invocation;
an internally consistent hash for the wrong section is rejected.

Drain-overlap assessment monitors early writer exit. It uses a unique private
copy of the pinned executable so detached successors remain attributable by exact
path, rechecks PID identity before signalling, and never kills by process basename.
If this host cannot inspect those identities, it refuses that assessment before
launch. Unexpected early exit aborts the schedule; expected cancel-after-reader
is explicitly labeled. Historical receipts retain the harness version that ran.

## Metric scopes and tests

Per-child wait4 records CPU and lifetime max RSS with explicit OS accounting of
reaped descendants. This is not simultaneous whole-tree peak or model/whole-stack
RAM. OS block operations are not I/O bytes. The fake HTTP server runs in owned
runner threads: handler CPU is recorded; separate server RSS is unavailable.
SQL query/transaction counts, exact SQLite/flock wait duration and exact product
hashed/parsed bytes are not currently collected. Inputs describe capacity, not
proof of how many bytes the product actually touched.

Real CLI tests use `PERF_LETOPIS_BINARY`, `PERF_OBSERVER_BINARY` and
`PERF_BASELINE_FILE` pointing to pinned product, existing native observer and build
receipt. Without them, explicit integration skips are reported. The ordinary
root Rust suite is not a substitute for these Python checks. On this host:

```sh
PERF_LETOPIS_BINARY="$PWD/.artifacts/issue-65/build-02/product/letopis" \
PERF_OBSERVER_BINARY="$PWD/target/release/deps/target_corpus-7451230d1f503f5d" \
PERF_BASELINE_FILE="$PWD/.artifacts/issue-65/build-02/build.json" \
  /opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py' -q
```

Discover the native observer via Cargo JSON as implemented by `build_observer`;
the hashed name above is a retained local example, not a portable fixed name.
Historical generator tests invoke bare `python3`: prepend the chosen interpreter's
directory to PATH when running them. [Report and remaining gaps](../../docs/zkit/letopis/checks/issue-65/report.md).

## Frozen optimization comparisons (#67)

`compare.py` keeps the original locked release receipt, a read-only copy of every
clean product input, executable and available symbols. It verifies archive hashes
before calls without requiring the working checkout to remain on the old revision.
`scenarios.verify_build` retains its existing checkout-bound single-revision contract.
An executable/hash alone is insufficient: comparisons require a frozen receipt and
its original release receipt and archives. These are local provenance records,
not signed compiler attestations; preserve the entire evidence directory.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --freeze-build \
  --source "$PWD" --evidence "$PWD/.artifacts/issue-67/before-unique"
# Freeze the optimized clean product separately after its checks pass.
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --stage timings \
  --before "$PWD/.artifacts/issue-67/before-unique/build.json" \
  --after "$PWD/.artifacts/issue-67/after-unique/build.json" \
  --nodes 60 --scenarios check --pairs 10 --timeout 120 --initial-timeout 600 \
  --evidence "$PWD/.artifacts/issue-67/check60-unique"
```

Every evidence path must be absolute, normalized and absent, with no symlink
parents. Fixtures use seed 65, 2,000 bytes per fragment, 30 elements per file and
disabled embeddings. Before and after must use the same compiler. Fixture bytes,
requests, configuration and independent expected outputs must match before timing.
Copy refreshes happen outside timing; initialized read scenarios reuse separate
stable before/after repositories and record projection revision counters.

Stages share the bounded defaults of 120 seconds per ordinary call, 600 seconds
per first publication and 1,200 seconds per stage, including setup. A shorter
`--stage-seconds` is an explicit recorded override. Each side's first publication
is recorded separately, once. An invalid publication stops initialized probes.
Attempts are flushed and synced before another call, including timeout, cancellation
and invalid output. Watchdog interruption is censored, never a successful latency.
Missing or rejected observations remain failures; medians exclude them. An
incomplete campaign exits nonzero and keeps its summary and failure receipts.

- `timings`: two warmups per side, then ten alternating before/after pairs.
  `--pairs` overrides this stage only; use at least ten successful samples for a
  feasible acceptance comparison. Defaults are check at 60, inspect/traverse/search
  at 1k. Explicit existing scenario names are accepted. Mutation/reindex attempts
  receive fresh copies rather than reusing their changed state.
- `initialization`: one first publication and up to three fresh attempts per
  requested audit/maintenance scenario and side. A timed-out side/scenario retains
  its censored receipt and skips later attempts; feasible sides retain three.
  Successful transports with invalid outputs remain visible as correctness failures.
  No automatic timeout retry occurs. Defaults: check, reindex, status, refresh_bulk.
- `contention`: three reindex/reader schedules per side; default reader is search.
  Read scenarios are check, inspect, traverse and search. Existing kernel-lock
  acknowledgement, process cleanup and overlap receipts remain authoritative.
  `overlap_assessed:false` means not assessed at this workload; it never establishes
  zero lock wait. CLI lifetime overlap is distinct from SQLite wait duration.
- `profiles`: three alternating ordinary/samply pairs per side and requested read
  scenario (default check/inspect/traverse), save-only at 1,000 Hz. Separate wrapper
  Git counters run once per side/scenario. Profiles, ordinary overhead controls and
  counters retain separate receipts; profile timings never enter ordinary series.
  Missing sampler/permissions are unavailable evidence, not empty success.

No stage automatically retries a censored operation. Reattempt once only after a
specific setup/product defect is resolved, under a new evidence path. The initial
Task 1 baseline uses the existing `run.py` survey with `--repetitions 10` for one
pre-change binary: two warmups and ten checks, without inventing an after build.
Direct child wait4 CPU/RSS retain their existing descendant-accounting scope;
concurrent whole-stack peak, exact lock waits and real-provider performance remain
unassessed. [Issue-67 evidence index](../../docs/zkit/letopis/checks/issue-67/evidence-index.md).

## Historical provenance campaigns (#108)

`historical.py` specializes frozen comparisons for 10k/100k knowledge elements.
It creates standalone repositories with seed 65, 30 elements per Markdown file,
2,000 bytes per fragment and disabled embeddings. Every element has the same
search marker and one external link. `shared` uses one 100-line source, whole-file
source addresses for every sixteenth element and otherwise line `1 + index % 64`;
`distinct` gives each element its own whole-file source with identical content.
Each shape owns a complete source tree and exact fixture hashes/counts. Their
different source cardinalities mean absolute times do not isolate one variable.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --freeze-build \
  --source "$PWD" --evidence "$PWD/.artifacts/issue-108/before-build"
/opt/homebrew/bin/python3.12 -B tests/performance/historical.py \
  --before "$PWD/.artifacts/issue-108/before-build/build.json" \
  --nodes 10000 --variant shared --stage timings \
  --evidence "$PWD/.artifacts/issue-108/before-shared-10k-timings"
# Repeat with distinct and 100000; each evidence directory must be new.
# Add --after /absolute/path/to/candidate/build.json for a matched comparison.
# Use --stage profiles under another evidence path for separate Samply captures.
```

Evidence paths must be absent, absolute and normalized, without symlink parents.
The baseline-only mode records no candidate statistics or speedup. By default,
both `check` and `status` run: first publication once per side (up to 600 seconds),
then two conditioning calls and three alternating ordinary pairs per scenario
(120 seconds per call). Each side reuses its own stable repository; generation
and publication precede ordinary samples. The 1,200-second stage watchdog includes
setup. Shorter explicit budgets and `--scenarios check status` are supported.

Attempts are flushed and synced before the next call. The first timeout, process
error or invalid output stops that side's current series without retry; an invalid
publication prevents that side's initialized series. Interruption stops all new
series. Incomplete campaigns exit nonzero and retain receipts, partial manifest
and summary. A censored attempt never becomes a successful latency.

Profiles use three alternating ordinary/Samply controls per side and scenario;
profile durations never enter ordinary medians. Missing sampling capabilities
remain failed/unavailable evidence. This runner uses no PATH Git wrapper.
Frozen source/build receipts, archived harness bytes, input hashes, requests,
independent count/output oracles, host load and measurement resource scopes remain
in the evidence directory. CPU/RSS inherit `measure_process`'s per-child wait4
scope; OS page cache and shared-host load are uncontrolled. These synthetic
disabled-provider runs do not establish production or real-provider performance.

Harness checks: `cd tests/performance && /opt/homebrew/bin/python3.12 -B -m unittest test_historical`.

### Supplemental unique-path hierarchy

`--variant distinct-sharded` keeps one distinct whole-file source per knowledge
element and the same source bytes as `distinct`, but puts 100 files in each
`sources/shard-NNNN/` directory (1,000 leaf directories at 100k). This separately
labelled control tests unique-path planning behavior without one huge flat Git
tree. The deterministic path is `sources/shard-{index // 100:04d}/source-{index}.txt`.
Before and after must use the same variant and equivalent hashes/requests.

Use a new `.../timings-100000-distinct-sharded` evidence directory and the same
120/600/1200-second budgets and three-pair schedule. This is a different input
shape, not a retry or successful completion of a censored flat-tree workload.
Do not compare absolute timings across the flat/sharded variants as an isolated
product speedup; paths, tree shape, and source-tree OID differ. Preserve original
timeout/cancellation receipts and the reason a supplemental control was needed.
