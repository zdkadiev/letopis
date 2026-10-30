# Model-free performance diagnosis (#65)

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
