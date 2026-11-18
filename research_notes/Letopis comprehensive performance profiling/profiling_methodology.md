# Letopis local macOS profiling tools and measurement methodology

**Execution update, 2026-11-17:** Later user authorization permitted baseline builds and a bounded CPU-profiler smoke, not a benchmark matrix. Both release builds succeeded at revision `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`; samply captured **127 usable stacks** and **125 resolved Letopis function names** from a disposable 300-element full-index operation. No privilege changes or installs were needed. Earlier reconnaissance-only statements below describe the initial research phase; this update supersedes their untested-build/permission/symbolization gaps. See [build validation](build_validation.md), [build metadata](evidence/build_metadata.json), and [actual capture validation](evidence/samply-index-smoke-validation.json).

The repeated-help smoke launched 100 processes but captured **zero samples**, explicitly retained as insufficient evidence. Actual successful capture used launch-mode samply at 1,000 Hz with `--save-only --no-open --unstable-presymbolicate`; macOS sample fallback was unnecessary. Rust names resolved; comprehensive source-line attribution, PID attach mode, detached workers, allocation history, timing distributions, and profiler overhead remain unassessed.

**Archive/survey continuation:** Both already-built executables and matching symbolized dSYM now have durable [archives and truthful runner-compatible receipts](evidence/build-archives-20261117-0046a6d-v1/README.md); original metadata preserved. Further authorized baseline timing/CPU/RSS surveys use existing canonical runner, not a new harness. Their actual results and coverage limits are recorded separately in [canonical_survey.md](canonical_survey.md); the earlier proposal below is not a claim that every proposed workload has run.

## Which tools are available, and what needs approval?

### Takeaway

Use **samply first**, as explicitly selected by the user; use macOS `sample` if samply attachment is blocked. Samply is already installed, so no CPU-profiler installation is needed. Current attachment permissions and Rust symbol quality remain untested: this reconnaissance intentionally did not launch workloads, build, benchmark, install, or change system permissions.

### Cited Findings

- On this host, discovery returned samply `/Users/zdkadiev/.cargo/bin/samply`, `/usr/bin/sample`, `/usr/bin/xctrace`, `/usr/bin/vmmap`, `/usr/bin/leaks`, cargo `/Users/zdkadiev/.cargo/bin/cargo`, and rustc `/Users/zdkadiev/.cargo/bin/rustc`. `command -v time` returned shell keyword `time`; use **`/usr/bin/time` explicitly**. Hyperfine was not found on PATH. — [Local discovery transcript](#local-discovery-and-help-excerpts)
- `samply --version` returned **0.13.1**. Its local `record --help` supports command launch, PID attachment, `--save-only`, `--rate` (default 1,000 Hz), `--iteration-count`, `--unstable-presymbolicate`, and `--symbol-dir`. System-wide `--all` and `--per-cpu-threads` are explicitly unsupported on macOS. — [Local help excerpts](#local-discovery-and-help-excerpts)
- Host metadata: macOS **26.6.2**, build **25G83**, **arm64**, model **Mac16,7**, **14 physical / 14 logical CPUs**, **51,539,607,552 bytes RAM (48 GiB)**. CPU model name and performance/efficiency core split were not queried. — [Local discovery transcript](#local-discovery-and-help-excerpts)
- `xctrace` exists as a shim, but `xctrace version` fails: full Xcode is required and active developer directory is `/Library/Developer/CommandLineTools`. Therefore Instruments is **not currently usable through the selected developer toolchain**; existence of its shim is not availability. — [Exact diagnostic](#local-discovery-and-help-excerpts)
- Existing accepted design records historical samply success outside a Codex sandbox and failure `Unknown(1100)` inside it; that establishes a historical environment restriction, not current permission status. Existing runner documentation also says samply requires execution outside that sandbox on this Mac. — [Accepted design, lines 115–124](../../docs/superpowers/specs/2026-10-30-issue-65-performance-design.md#stage-b-explain-the-expensive-cases); [Runner documentation](../../tests/performance/README.md#profilers-and-causal-controls)
- Project pins Rust **1.95.0**; performance harness builds optimized `release` with `CARGO_PROFILE_RELEASE_DEBUG=1`, archives the executable, copies adjacent `.dSYM` if present, and records hashes/toolchain/build command. Distribution profile instead inherits release and enables LTO, one codegen unit, abort-on-panic, and stripping. These are different build identities and must not be conflated. — [Toolchain](../../rust-toolchain.toml); [Build helper, lines 63–94](../../tests/performance/scenarios.py); [Distribution profile, lines 81–88](../../Cargo.toml); [Make release](../../Makefile)
- No repository `.cargo/` directory was found. No explicit frame-pointer or split-debug setting was found in the inspected root manifest and performance harness. Ambient Cargo/Rust environment overrides, global Cargo config, existing binary debug data, and actual installed compiler version were not inspected. — [Manifest](../../Cargo.toml); [Build helper](../../tests/performance/scenarios.py)

### Inferences

- **No installation approval needed for samply/sample/time/vmmap/leaks availability.** After execution authorization, perform one owned-fixture smoke capture first. Validate nonempty stacks, expected Letopis PID, resolved Rust names, profile completeness, and symbol sidecar before launching the campaign. Presence plus `--help` does not prove Mach task attachment works.
- If samply is blocked, retain exact error and attempt `sample` on an exact owned PID. Do not infer fallback will evade the same OS attachment restriction. If both fail, report CPU stacks unavailable; do not request `sudo`, alter SIP, weaken security settings, or install Xcode silently.
- Preserve executable and matching debug artifacts together. Existing optimized release/debug=1 path is simplest starting point; demangled names, source locations, and inline attribution must be checked in actual output rather than promised. Do not add frame-pointer flags preemptively. If needed later, identify a separate diagnostic build and compare overhead against the original build.
- If production-distribution fidelity matters, measure the actual dist binary separately. A release/debug=1 profile does not automatically explain LTO/stripped dist behavior. A future symbol-preserving dist-equivalent build needs explicit build configuration/provenance, not an undocumented substitution.
- Full Xcode installation/selection would be additional setup outside this assignment. It is not prerequisite for the recommended CPU/time/RSS campaign. Chronological heap allocation tracing remains a named gap unless an approved working allocation profiler is selected.

### Gaps

- Current attachment permission, symbolization, unwind quality, profile child coverage, and profiler overhead were deliberately not tested.
- Clarification about profiler choice is resolved: user selected samply, macOS sample fallback.
- Actual installed rustc version was not invoked; 1.95.0 is the project pin, not a verified installed-version claim.

### Local discovery and help excerpts

Captured during this read-only research on 2026-11-17. Commands only queried metadata/help/manuals; no target process was profiled.

```text
$ for tool in samply sample time xctrace vmmap leaks hyperfine cargo rustc; do command -v "$tool" || true; done
/Users/zdkadiev/.cargo/bin/samply
/usr/bin/sample
time
/usr/bin/xctrace
/usr/bin/vmmap
/usr/bin/leaks
/Users/zdkadiev/.cargo/bin/cargo
/Users/zdkadiev/.cargo/bin/rustc

$ sw_vers
ProductName: macOS
ProductVersion: 26.6.2
BuildVersion: 25G83

$ sysctl -n hw.machine hw.model hw.physicalcpu hw.logicalcpu hw.memsize
arm64
Mac16,7
14
14
51539607552

$ samply --version
samply 0.13.1

$ samply record --help
Usage: samply record [OPTIONS] [COMMAND]...
-r, --rate <RATE>                 Sampling rate, in Hz [default: 1000]
--iteration-count <ITERATION_COUNT> How many times to run the profiled command [default: 1]
--unstable-presymbolicate         Emit .syms.json sidecar file containing gathered symbol info...
-s, --save-only                   Do not run a local server after recording
--symbol-dir <SYMBOL_DIR>         Extra directories containing symbol files
-p, --pid <PID>                   Process ID of existing process to attach to
-a, --all                        Profile entire system (all processes). Not supported on macOS
--per-cpu-threads                 Create a separate thread for each CPU. Not supported on macOS

$ /usr/bin/sample -h
[invalid usage]: no process id or name or memgraph specified
Usage: sample <pid | partial-process-name> [duration [samplingInterval]] [options...]
Specify the duration in seconds; the default is 10 seconds.
Specify the samplingInterval in milliseconds; the default is 1 millisecond.
-mayDie reads symbol information right away
-fullPaths show full paths to source files if debug info is available
-f/-file <filename> specifies where results should be written

$ xctrace version
xcode-select: error: tool 'xctrace' requires Xcode, but active developer directory '/Library/Developer/CommandLineTools' is a command line tools instance

$ /usr/bin/vmmap -h
vmmap: Gives you an indication of the VM used by a process
-summary only print overall summary, not individual regions
-stacks show region allocation backtraces if target process uses MallocStackLogging

$ /usr/bin/leaks -h
leaks: Search through a process for leaked memory.
Usage: leaks [options] --atExit -- <command-and-arguments>
--noContent Do not print or save any allocation content descriptions
--atExit launch the specified command and run leaks when that process exits.
```

The `time -h` probe produced no useful help; authoritative local syntax below comes from `MANPAGER=cat man 1 time | col -b`.

## What do CPU, timing, and memory measurements actually establish?

### Takeaway

Collect ordinary end-to-end timings, user/system CPU and lifetime peak RSS independently of CPU stack captures. RSS, VM snapshots, allocation history, and leak detection answer different questions; none can substitute for the others or silently cover detached background work.

### Cited Findings

- macOS `sample` periodically suspends the process, records **all thread stacks**, and resumes it. Local manual warns that functions between sample points may be missed. `-mayDie` collects symbol locations immediately. This is a generic native stack sampler, not a Rust-specific profiler; source paths require debug information. — [Local manual: `sample(1)`](file:///usr/share/man/man1/sample.1)
- Samply is the project’s primary CPU profiler; supplementary sample captures are labeled wall-time stack observations, explicitly not exact blocked-time percentages. Existing sampler uses save-only at 1,000 Hz with presymbolication and rejects missing/truncated/zero-usable-stack profiles. — [Sampler implementation, lines 22–101](../../tests/performance/profile.py); [Accepted design](../../docs/superpowers/specs/2026-10-30-issue-65-performance-design.md#stage-b-explain-the-expensive-cases)
- Existing CPU summarizer weights usable stacks by `threadCPUDelta`, supports PID filtering, and explicitly records that inclusive costs overlap and wall percentages are unavailable. Raw stack counts alone are not necessarily sampled CPU shares. — [Summarizer, lines 104–130](../../tests/performance/profile.py)
- `/usr/bin/time [-al] [-h | -p] [-o file] utility ...` reports elapsed, user, and system time; `-l` also prints resource usage. Local manual example includes maximum RSS and peak memory footprint as separately named fields, plus instructions/cycles; this research has not verified their populated values or exact accounting for Letopis. — [Local manual: `time(1)`](file:///usr/share/man/man1/time.1)
- macOS `getrusage(2)` defines `ru_maxrss` in **bytes**, not Linux KiB. It defines block counters as filesystem input/output operations and notes cached data is charged only on first actual I/O. Thus block counts are not disk bytes and cannot quantify all logical file reads. — [Local manual: `getrusage(2)`](file:///usr/share/man/man2/getrusage.2)
- Existing Python measurement uses per-child `wait4`, labels scope `wait4_direct_child_and_OS_accounted_reaped_descendants`, and labels RSS `wait4_lifetime_max_not_simultaneous_tree_peak`. Timer includes spawn, input/output collection, and owned-group cleanup. Its selector polls at up to 5 ms intervals. — [Measurement implementation, lines 17–57, 79–124, 146–174](../../tests/performance/measure.py)
- CLI completion may launch background work before returning its response. Detached launcher uses `setsid()`, null standard streams, and does not wait; child is reparented after parent exits. Such work can outlive pipe EOF and escape cleanup limited to the foreground process group. — [CLI finish](../../src/cli/response.rs); [Detached process](../../src/process/detached.rs)
- Existing contention documentation already addresses detached successors using a unique private executable path and identity checks; identity inspection failure stops assessment rather than killing by basename. Existing runner does not collect whole-tree simultaneous peak RSS, exact lock waits, SQL counts, or exact product byte counts. — [Runner scopes and ownership](../../tests/performance/README.md#metric-scopes-and-tests); [Drain-overlap documentation](../../tests/performance/README.md#profilers-and-causal-controls)
- `leaks` scans for unreferenced malloc buffers and conservatively treats reachable buffers as in use. `--atExit` automatically enables `MallocStackLogging=lite`; allocation stack logging is therefore instrumentation, not ordinary baseline execution. `--noContent` avoids descriptions that could reveal confidential process content. — [Local manual: `leaks(1)`](file:///usr/share/man/man1/leaks.1)
- `vmmap -summary` describes a process’s current mapped memory, and `-stacks` requires MallocStackLogging. No allocation timeline or lifetime peak follows from a single VM snapshot. — [Local vmmap help](#local-discovery-and-help-excerpts)

### Inferences

- Report metric names with units, process scope, method, and limitations: foreground latency; user CPU; system CPU; accounted CPU; lifetime peak RSS bytes; optionally memory-footprint field kept separate. Never label RSS as “allocated bytes,” memory footprint as interchangeable with RSS, or summed process peaks as simultaneous family peak.
- Main-process CPU may be low because of I/O, locks, provider waits, or scheduling. Sample stacks can suggest waits, but neither CPU percentages nor sample counts establish exact SQLite/flock/provider wait durations.
- Define **two endpoints** when background embedding/maintenance is enabled: foreground response/exit and completed background workload/queue quiescence. Keep provider and detached-worker metrics separate; ordinary `time`/wait4 of foreground is not full-workflow resource accounting.
- Existing process-group cleanup does not contain `setsid()` descendants. Reuse explicit identity/ownership machinery for background scenarios; otherwise fail closed before starting them. Do not assume samply automatically captures detached workers: inspect recorded PIDs and lifetimes and report missing coverage.
- Supplement peak RSS with vmmap snapshots for sufficiently long commands and separately instrumented `leaks --atExit -- ...` runs when authorized. Leak-free output means no leaks detected by that run’s scanner, not no retained memory, no high allocation churn, no unsafe memory bug, or no background leak.
- Full allocation/deallocation timeline, allocation volume/count, and retained bytes by Rust call site are **not established** by installed CPU profilers, RSS counters, or one vmmap snapshot. Consider approved full Xcode Allocations or separately approved allocator instrumentation only if this requirement is mandatory; do not quietly add allocator dependencies/product changes.

### Gaps

- Exact detached-descendant accounting and sampler retention must be verified experimentally in approved execution, including whether sampling continues after foreground exit.
- No current allocator choice audit, vmmap/leaks target-permission test, heap-growth trace, leak report, hardware counter validation, or memory-footprint validation was performed.
- macOS manuals are locally installed primary sources. Their file URLs identify manual locations; `man` rendering is the verified source, not a claim that every machine shares these paths.

## How should a reproducible campaign run without misleading results?

### Takeaway

Reuse existing pinned-fixture/per-invocation evidence harness where applicable, extending coverage only after auditing current command/load contracts. Separate plain timing survey, sampled CPU explanation, and instrumented memory diagnostics; reset mutation inputs and report uncontrolled OS caching honestly.

### Cited Findings

- Existing design prescribes two conditioning calls plus 20 fresh-process measurements for cheap repeatable cases, three starting observations for expensive refresh/reindex/100k cases, and no p95 below 100 successes or p99 below 1,000. These are project reporting conventions, not confidence guarantees. — [Accepted design, lines 90–105](../../docs/superpowers/specs/2026-10-30-issue-65-performance-design.md)
- Existing harness alternates ordinary/samply pair order, prepares equivalent fixtures per run, and keeps resets/correctness outside timers. Sampled receipt measures the sampler invocation, so duration includes profiler launch/finalization and resource totals may include profiler activity; it is not pure product CPU/RSS. — [Profile pairing, lines 225–246](../../tests/performance/profile.py); [Sampler invocation, lines 22–49](../../tests/performance/profile.py)
- Runner documentation warns copies change inodes, requiring projection refresh before mutation; it distinguishes first publication, initialized state, and conditioned calls. Existing reusable fixtures require synthetic standalone repositories with embeddings disabled; this restriction is incompatible with blindly passing arbitrary user knowledge bases into that path. — [Runner setup](../../tests/performance/README.md); [Fixture reuse checks, lines 47–60](../../tests/performance/scenarios.py)
- Existing retained findings explicitly flag insufficient samples for tiny commands: init had 60 and config one. Inclusive stack shares were not treated as wall-time shares. — [November 10 findings](../../letopis/development/performance-bottlenecks.md#command-profiles-2026-11-10)
- Existing runner preserves partial/censored attempts, uses fresh evidence directories, checks correctness, and explicitly separates synthetic/fake-provider behavior from real-model and production relevance. Historical profile windows are never treated as successful end-to-end latency. — [Runner documentation](../../tests/performance/README.md); [Window receipt, lines 249–271](../../tests/performance/profile.py)

### Inferences

Proposed campaign, **not executed**:

1. **Freeze identity and scope.** Record Git revision/dirty state, exact archived executable and symbols with hashes, build flags/toolchain, harness version, OS/CPU/RAM, profiler version/rate, power mode, ambient load, command argv/stdin, output-capture method, and environment allowlist. Never copy credentials. Initial repository reconnaissance showed branch `main...origin/main` and only untracked `reports/`; preserve this work.
2. **Make coverage explicit.** Inventory every current command/flag family and its preconditions; do not call historical harness coverage “every command.” Include tiny/no-work paths and useful busy paths separately, unchanged vs changed projection, first publication vs incremental/full rebuild, disabled/fake/real provider modes only as authorized, foreground vs background completion, and correctness outcomes.
3. **Vary workload shape, not just node count.** Use deterministic small/1k/10k/100k scales where feasible, preserving knowledge bytes, files/elements, graph edges/fanout, source cardinality and source sharing, query selectivity, traversal bounds, Git tree size/history/packing, and edit volume in manifest. Fix all but one variable in causal comparisons; real approved knowledge bases remain separate workload families, not interchangeable with synthetic fixtures.
4. **Reset fixture per mutation.** Fixture generation/copy/refresh/pre-edit setup occurs outside the measured interval; measure actual intended first-use operation only when first use is the scenario. Verify expected output and post-state after each run. Never time an edit once then accidentally benchmark its no-change successors.
5. **Plain timings first.** Launch exact archived binary directly, not `cargo run`, shell pipeline, or profiler wrapper. Use existing high-resolution monotonic+wait4 receipts; `/usr/bin/time -l -p -o <fresh-file> <binary> ...` is a simple independent cross-check after approval. Include spawn/serialization/output in CLI latency by design; document harness cleanup/pipe endpoint. For millisecond commands, inspect 5 ms polling/output overhead and compare an independent timer before interpreting fine differences.
6. **Cache honesty.** Label application state explicitly (absent, initialized, changed, unchanged), and OS state separately. “First use” of app cache is not cold filesystem cache; newly copied/generated fixtures have just warmed page caches. Warm conditioning does not establish all files resident. Use “OS cache uncontrolled” unless genuinely controlled. No privileged purge or unrelated workload interference.
7. **Repeat and balance.** Start with project defaults above; report individual values, successes/failures, median/range and sample count. Alternate AB/BA for profile overhead and before/after comparisons; use recorded randomized/block-balanced scenario order for broader matrices. Never pool first-use and warmed timings or include censored failures as zeros. At high cost, bounded three-run summaries are honest but weak, not tail-latency evidence.
8. **Separate CPU profiles.** Starting template after approval: `samply record --save-only --rate 1000 --unstable-presymbolicate --output <fresh-profile.json.gz> <archived-binary> <args>`. Same binary, config and equivalent fixture as plain control; retain raw profile and `.syms.json`, PID list, usable samples, sampled CPU duration and symbol validation. Pair at least three ordinary/profiled attempts where feasible; report observed wrapper-inclusive elapsed ratio/difference, not an assumed universal overhead percentage.
9. **Short commands.** Prefer launch-mode samply over PID attachment to avoid missing startup. At 1 kHz a few-millisecond process offers very few observations; attachment delay further worsens sample. No usable samples means insufficient evidence, not zero cost. Samply supports repeated launch with `--iteration-count`, but never repeat mutating commands without reset. Separately prepared fresh invocations may be aggregated with per-invocation counts/uncertainty; do not introduce an artificial in-process loop and call it real CLI latency. Longer realistic workloads can explain scalable hot paths, not necessarily tiny-command startup.
10. **Fallback wall stacks.** Existing owned-PID template: `/usr/bin/sample <exact-owned-pid> 3 1 -mayDie -file <new-sample.txt>`. Long enough commands only; retain actual capture interval and lifecycle. A three-second window inside a long command is partial coverage, not full-command percentages. Never use loose process-name matching against a user’s live processes.
11. **Separate memory diagnostics.** Plain wait4 yields lifetime RSS; vmmap snapshots belong to separate diagnostic runs. Optional `leaks --noContent --atExit -- <archived-binary> ...` is instrumented, separately reset and timed only as diagnostic overhead, not mixed into baseline distributions. Do not save user-memory content by default. Whole-family live RSS polling, if later added, must be labeled sampled/possibly shared-page double-counted, not exact physical peak.
12. **Interpret limits and stop rules.** Timeouts preserve attempted duration/censored outcome; profile window success is not workload completion. Stop runaway growth at explicit budgets, do not extrapolate missing 100k results. Sampling percentages are observed CPU attribution, not wall time, nonoverlapping components, allocation shares, or proof of causality. Confirm expensive hypotheses with one-variable controls; report unsupported metrics as unavailable.

### Gaps

- No timings, CPU profile, memory numbers, overhead estimate, or “current bottleneck” is claimed by this reconnaissance.
- Current hardware is known, but thermal state, power mode, competing workload, permissions and target symbol quality remain execution-time metadata.
- Existing harness is useful infrastructure, not automatic full command/load/background/allocation coverage. Campaign approval must settle workload breadth, fixture sizes, repetition/time budgets, allowed knowledge-base copies, and any fake or real provider access before execution.
- CPU sampling setup needs no install. If “full memory profiling” means allocation chronology/by-call-site volume rather than RSS plus snapshots/leak checks, additional tool selection/setup approval is required; current Xcode limitation blocks recommending Instruments as immediately runnable.
