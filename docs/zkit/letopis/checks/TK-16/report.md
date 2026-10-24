# TK-16 — Local RAM assessment

> Status: local measurements complete; independent review pending.
> Formal 4 GB hardware/budget acceptance: **not assessed**; G-1 remains blocked.
> Date: 2026-10-24. Base main@ccb15f0; branch codex/tk-16.
> Worktree: /Users/zdkadiev/.codex/worktrees/tk-16/letopis.

## Scope and result

User directed “ok just assess ram usage”. This measures the available Mac:
macOS 26.6.2, Mac16,7, 14 CPUs, 51,539,607,552 RAM bytes (48 GiB).
[Specification](../../../../superpowers/specs/2026-10-24-tk-16-local-ram-design.md)
and [plan](../../../../superpowers/plans/2026-10-24-tk-16-local-ram.md)
preserve the narrowed scope; the [original task](../../roadmap/epics/EP-7-early-validation/tasks/TK-16-early-memory.md)
still requires 4 GB hardware and complete memory accounting.

Across all five windows, including the failed overlap, maximum observed target
physical-charge sum was **379.097 MB**; maximum summed RSS was **739.312 MB**.
Those maxima occur in different windows. Including the recorded measurement
coordinator/observer RSS at the same instant reached **1,187.430 MB** during
fresh backlog loading. These are observations, not a full unique-RAM bound.

| Window | Physical-charge sum MB | Summed RSS MB | With recorded orchestration RSS MB | Complete/native samples |
|---|---:|---:|---:|---:|
| Populated: first relocated-fixture search | 314.101 | 739.312 | 776.815 | 79/79 |
| Populated: warm search | 273.518 | 613.810 | 651.575 | 6/6 |
| Fresh backlog: initial load | 379.097 | 732.119 | 1187.430 | 165/165 |
| Backlog: first overlap (query failed) | 248.467 | 514.998 | 743.096 | 72/73 |
| Backlog: repeated overlap (query succeeded) | 249.303 | 520.110 | 555.057 | 71/72 |


Each column is its own observed maximum; do not add column maxima together.
The inclusive RSS column is computed from simultaneous app + recorded-tool rows,
not the sum of separate peaks. Persistent Ollama GUI/server/model runners and
owned CLI/Git children are target processes. Python orchestration, the Rust
coverage observer and time wrappers are separate measurement overhead. Transient
ps/footprint utility processes are excluded. The largest recorded instrumentation
RSS alone was 591.561 MB. All 395 native samples remain, with 393 complete samples
used for physical-charge/group maxima; missing/exited-PID samples never become zero.

## Inputs, coverage and preservation

Same TK-15 seed 15: 100,000 knowledge nodes, 100,000 source addresses, 500,000 edges;
100,000 complete marked fragments of exactly 2,000 UTF-8 bytes, total 200,000,000;
3,334 Markdown files and 1,000 source shards. Both fixture manifests are byte-
identical; all 4,334 input hashes match in each. No vector acceptance data was seeded.
The populated repository was copied under nonblocking read-only index/embed/runner
lock leases. Original cache, vector database and config bytes still match all three
pre-copy hashes. Original TK-15 stores and binaries remain untouched.

Existing release product and test observer are reused with their pinned hashes.
Model: `embeddinggemma:300m-qat-q4_0`; digest
`101341d65c2ccbf23f16650b79d30b9fca94a45ffa09a9984c600157b81a58df`.
Config identity:
`0cbbd152c2bdaa72c3f1ed0f087ec6fb97f4752286b54e213901a6e217646bb5`.
Local Ollama API only; no download, model substitution or paid provider.
An unrelated Qwen model expired naturally before the windows. Every window's
model inventory shows only the selected Gemma model; none is marked contaminated.
The shared provider was never killed or explicitly unloaded.

| State | Current / missing knowledge vectors | Eligible / failed / exhausted | Dimensions |
|---|---:|---:|---:|
| Populated before and after search | 100,000 / 0 | 0 / 0 / 0 | 768 |
| Fresh loaded backlog | 0 / 100,000 | 100,000 / 0 / 0 | null |
| After first 45-second drain | 1,251 / 98,749 | 98,749 / 0 / 0 | 768 |
| After repeated 45-second drain | 2,458 / 97,542 | 97,542 / 0 / 0 | 768 |

All observations retain 100k/100k/500k counts, revision 1 and zero invalid files.
Populated nonempty searches returned five results and left zero query_error rows.
The first relocated-fixture search took 48.143 s and the warm search 3.667 s;
the first includes revalidation of copied file inodes, not a pristine cold-cache
benchmark. Initial backlog load took 100.955 s. The repeated owned drain was
interrupted with SIGINT near 45 s; its recorded exit -2 is intentional, not an OOM.
There was no fresh 100k embedding run.

## Observed workflow failure

**One of two overlapping nonempty searches returned `internal_error`** with the
safe message “cannot search knowledge” (about 0.38 s). The first owned drain
continued until its planned interruption and produced 1,251 current vectors.
Two idle triage searches, one nonempty and one empty, then succeeded under an
embedding barrier; the repeated overlap also succeeded and returned five results.

This is an intermittent observed failure, not a diagnosed root cause or a fixed
product bug. The failed window remains included in all-window RAM maxima. Raw
error response, timing, process samples and coverage are retained. Its original
probe asserted before summary emission; that phase's exact exit code was not
retained and is not invented. The repeat stores explicit scenario outcome and
signal-PID evidence. No product repair or redesign was attempted under this RAM-
only request. This failure needs investigation before treating concurrent drain
and search as reliably validated.

## Meter validation and limits

Native ps supplies RSS; footprint supplies per-process physical charges and a
native group footprint, both retained separately. Physical footprint treats clean
mappings and compressed/swapped memory differently from RSS and includes unified
CPU/GPU objects on Apple silicon: [Apple explanation](https://developer.apple.com/videos/play/wwdc2022/10106/).
RSS sums can double-count shared pages. Do not add size_vram to either counter.
Neither the reported charged footprint nor the selected-process RSS sum measures
all attributed OS file cache/shared buffers. Short-lived children may evade sampling.
The loop delays 0.5 s between captures; actual timestamps, not a guaranteed fixed
sample rate, are retained. Sampling overhead means elapsed times are not latency
benchmarks. MB here is decimal; the unchanged budget is 1,000,000,000 bytes.

Live calibration touched 67,108,864 bytes (64 MiB), increasing physical charges by
67,190,856 bytes; macOS time reported maximum RSS 80,478,208 bytes. The initial
calibration exposed one-shot JSON lacking the sampled wrapper and stopped before
application workloads. Its failure/source and cleanup timing remain in run-01;
the helper exited and was checked gone before the corrected run.

Three captured-native-fixture checks pass for one-shot/series formats, strict byte
units and unavailable values. Three disposable semantic controls were caught:
**3 caught / 0 missed / 0 timeout / 0 unviable**, each restored GREEN. No product
mutation sweep or unchanged root suite was rerun. Original 146 product/build/test
hashes still equal the previously verified main snapshot (make verify: 481 passed,
0 failed, 5 ignored per profile; 49 Python 3.12 tests). These are reused historical
gates, not fresh tests in TK-16. No src/dependency/schema/interface/composition
changes exist; hexagonal module boundaries remain unchanged.

## Reproduction and evidence

Operational scripts are local ignored artifacts, not a new product profiler:
`.artifacts/tk16/probe.py`, `repeat.py`, `check_probe.py`. Exact commands, source
versions and SHA-256 hashes are retained in run-01/run-02/run-03. Use supported
Python 3.12 on PATH for parent and child CLIs. Outputs are exclusive-created;
reproduction needs fresh paths and must preserve old receipts and the original
TK-15 corpus. Key outputs: assessment.json, independently recomputed analysis.json,
phase native/sample JSON, command outputs, initial load and before/after observer
receipts, source-copy proof, parser controls and live calibration.

Evidence snapshot: 1286 files in `.artifacts/tk16/evidence-manifest.json`,
SHA-256 `bf6e4f884a7282db3e21621a197b8030942b66f85a57a4be2c6d86028fe54e25`. Corpus databases are excluded from
that raw-tool snapshot and verified separately. All raw evidence stays in this
worktree; no merge, push or GitHub issue update is part of this assessment.

## Controller ruling

Retain failed scenario outcomes alongside valid memory samples rather than aborting
receipt emission; failure must stay observable. Cost if wrong: RAM figures could
be mistaken for proof that the workflow succeeds. Scenario success is recorded
separately, the initial failure stays open, and G-1 remains not assessed.

## Formal verdict

Local RAM observations are available. **TK-16 acceptance is blocked/not assessed**:
no 4 GB hardware run, no complete OS-cache/shared-buffer attribution, sampled
rather than absolute concurrent maxima, and one unresolved overlap failure.
No conclusion that the full stack fits 1 GB or that G-1 passes follows from these
numbers. Independent local-evidence review and Engram final recording remain.
