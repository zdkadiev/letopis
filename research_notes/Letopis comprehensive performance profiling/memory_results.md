# Native memory diagnostic results — 2026-11-17

## Bounded live extension — three 10k captures

The subsequent extension supersedes the coverage gap in the original phase below; original receipts and their missing RSS guard remain unchanged. All three directly measured instrumented products exited 0 and passed independent result and projection-count oracles. [Completion and receipts](evidence/native-memory-extension-20261117-v1/completion.json).

| Command | Snapshot after launch, approximate | Prefix allocated heap + VM high-water / live at snapshot | Product wait4 peak RSS | Product user / system CPU |
|---|---:|---:|---:|---:|
| check | 0.187 s | 26.9 / 6.5 MB | 71,991,296 B | 0.629 / 0.937 s |
| inspect | 0.133 s | 4.4 / 4.0 MB | 24,002,560 B | 0.759 / 0.825 s |
| index --full | 0.738 s | 30.7 / 30.6 MB | 85,082,112 B | 7.474 / 1.476 s |

Snapshot times come from native report launch/snapshot timestamps, not exact input EOF times. Prefix high-water is allocator-logged heap **plus VM**, not RSS or the later full-process high-water. Product RSS is wait4 lifetime peak under allocation logging and inspection, with OS-accounted descendants; it is not an uninstrumented baseline or simultaneous process-tree peak. Three observed product RSS values were below the **1 GiB post-observation stop threshold**. [Check](evidence/native-memory-extension-20261117-v1/capture-check/receipt.json), [inspect](evidence/native-memory-extension-20261117-v1/capture-inspect/receipt.json), [index](evidence/native-memory-extension-20261117-v1/capture-reindex/receipt.json).

- **Check:** prefix high-water included 7,864,320 B in an edge vector and 5,767,168 B in a node vector, allocated under `GraphDiagnosticsService::find_problems` and storage reads. This supports the graph-materialization hypothesis for this synthetic fixture; it does not establish later full-command peak ownership. [Stacks](evidence/native-memory-extension-20261117-v1/capture-check/highwater.txt); [source](../../src/service/graph_diagnostics.rs).
- **Inspect:** 2,146,304 B reported on the signal-listener thread creation path; 559,104 B on SQLite page allocation; 379,968 B in Git object-write compression initialization. The report includes VM allocations, so thread stack reservation is not necessarily resident heap. [Stacks](evidence/native-memory-extension-20261117-v1/capture-inspect/highwater.txt); [Git source](../../src/git/repository.rs).
- **Full index:** 8,047,680 B across 9,960 allocations attributed to `BasisPreflight::add_file` BTree construction; 5,565,776 B across 4,166 allocations under `EffectiveGraph::from_storage` BTree construction. These allocations coexist at the logged prefix high-water; allocation stacks identify origin, not definitive current ownership. [Stacks](evidence/native-memory-extension-20261117-v1/capture-reindex/highwater.txt); [source](../../src/service/document_refresher.rs).
- Offline no-content leak analysis reported **zero leaks at each of the three snapshots**. This is not proof of no leaks or no retained reachable memory. [Check](evidence/native-memory-extension-20261117-v1/capture-check/leaks-offline.txt), [inspect](evidence/native-memory-extension-20261117-v1/capture-inspect/leaks-offline.txt), [index](evidence/native-memory-extension-20261117-v1/capture-reindex/leaks-offline.txt).

The one-off [launcher](evidence/native-memory-extension-20261117-v1/launcher.py) used the existing `measure_process` and a nonblocking `on_spawn` thread; the owner sent JSON and EOF normally and retained the product's actual exit and wait4 usage. A formatter added raw user/system/max-RSS fields without replacing existing metrics. Before attach, `ps` checked the direct parent and unique private executable path, with cancellation if the product had completed; the check-to-attach interval still admits a race. The copied executable hash matches the pinned archive and its matching dSYM was copied alongside. `MallocStackLoggingNoCompact=1` ran only in each product; `leaks -noContent -fullStackHistory --outputGraph=<path> <pid>` and offline tools used no-content modes. Full-history means **available history only through each live snapshot**, not full lifetime allocation traffic.

The coordinator's resource ruling under user autonomy—not an explicit user-set budget—allowed the **4 GiB** evidence soft cap only while free space remained at least **20 GiB**, otherwise 2 GiB. The first guarded combined evidence size was 2,164,133,888 B with 62,580,785,152 B free; the last recorded checkpoint, before final offline leak analysis, was 2,498,408,448 B with 62,242,656,256 B free. [Guard records](evidence/native-memory-extension-20261117-v1/guards.jsonl). Later reported values of 2,498,420,736 B evidence and 62,239,973,376 B free have no retained raw receipt and are not independently verified. Stage completed in 36.1 s, product cap 120 s per case; no hard instantaneous RSS guard. No baseline, sampler, provider, build, or real-repository workload was run.

## What did the native snapshot show?

### Takeaway

One **valid, early live 10k `index --full` snapshot** provides allocation-stack evidence, not a complete-command high-water mark. `MallocStackLoggingNoCompact=1` permitted a 1.30 MB full-history memgraph; normal `MallocStackLogging=YES` attempts were reported as lite. The product completed with direct exit 0 and passed the independent projection oracle. [Product receipt](evidence/native-memory-20261117-v1/nocompact-10k-index/product-receipt.json); [graph capture](evidence/native-memory-20261117-v1/nocompact-10k-index/leaks.txt).

### Cited findings

- Snapshot timestamp 26 ms after launch, while `EffectiveGraph::from_storage` was reading nodes. Logged allocated heap + VM high-water **12.5 MB** and cumulative allocations **52.9 MB** through that snapshot only; 240,074 allocation and 189,551 deallocation operations reported. They are neither the full invocation's high-water nor total lifetime churn; full history covers only the available early prefix. [High-water report](evidence/native-memory-20261117-v1/nocompact-10k-index/highwater.txt).
- Largest reported high-water stack: **5,767,168 bytes**, one `RawVec::grow_one` allocation under `CacheStorage::nodes_get` → `EffectiveGraph::from_storage`. SQLite `pcache1Alloc` under the same node read accounted for a separate 1,192,464 bytes over 273 calls. These are allocation stacks at one observed moment, not proof of persistent retention or an optimized outcome. [Stack report](evidence/native-memory-20261117-v1/nocompact-10k-index/highwater.txt); [source](../../src/service/document_refresher.rs); [cache](../../src/storage/cache.rs).
- Offline `heap -s -noContent` showed **11,453,856 bytes in 50,987 malloc nodes**, including approximately 7.1 MB grouped by Rust `RawVec` realloc and 3.2 MB grouped by SQLite allocation. Native type labels are backtrace-derived, not exact Rust ownership. [Heap](evidence/native-memory-20261117-v1/nocompact-10k-index/heap.txt). `vmmap` showed physical footprint 16.2 MB at snapshot, writable virtual 117.3 MB, resident 15.3 MB; reservations are not RSS. [VM map](evidence/native-memory-20261117-v1/nocompact-10k-index/vmmap.txt).
- Offline `leaks -noContent` reported **0 leaks / 0 leaked bytes** in the one early live graph. It does not establish absence of leaks, excessive reachable retention, or later transient peaks. [Leak report](evidence/native-memory-20261117-v1/nocompact-10k-index/leaks-offline.txt). [Live call tree](evidence/native-memory-20261117-v1/nocompact-10k-index/live.txt) notes missing stacks for earlier allocations; logging coverage is imperfect.
- Separate stock-release uninstrumented baseline at 10k reindex median **77.23 MiB RSS**, not comparable as a matched measurement of this debug1 instrumented prefix. [Baseline](canonical_survey.md#10000-elements).

### Inferences

- Observed node-vector and SQLite page-cache allocations support MEM-01/MEM-15 as meaningful early-prefix allocation owners for this fixture. They do not establish peak dominance, full-process scaling, or leak causation. [Hypotheses](memory_hypotheses.md).

### Gaps

- Original phase had no check or inspect heap captures; **this gap is superseded** by the later three live captures above. Status, traverse and search remain without valid heap captures. No valid at-exit graph, full-invocation allocator peak, or full lifetime churn was established. The 1k first capture was rejected before product execution because installed tool requires `--outputGraph=PATH`, not split arguments. Corrected at-exit execution returned valid JSON and oracle but `leaks` exited 255: target was lite and full history refused. [Usage error](evidence/native-memory-20261117-v1/reindex/combined.stderr); [lite refusal](evidence/native-memory-20261117-v1/capture-reindex/combined.stderr); [corrected product](evidence/native-memory-20261117-v1/capture-reindex/product.json). Running `/usr/bin/env` after `-atExit --` returned wrapper 0 and valid product JSON but no graph or leak report; wrapper success is not product exit or successful diagnostics. [Evidence](evidence/native-memory-20261117-v1/capture-env-reindex/receipt.json). A direct 10k `MallocStackLogging=YES` live attach also refused full history as lite. [Error](evidence/native-memory-20261117-v1/live-10k-index/leaks.txt).
- 120 s was applied to each `leaks` invocation and each offline analysis; direct product `wait` has its own 120 s cap after attachment, not one combined 120 s cap. Direct live launcher did **not** record post-process wait4 peak RSS; snapshot footprint is not peak RSS. Thus requested 1 GiB post-observation guard was not actually enforceable for those direct runs, and cannot be claimed passed. No additional capture was started after this gap was recognized. No hard instantaneous resource protection. No signing, SIP, privilege, install, source, Cargo, or network changes.

## Was the existing resource accounting calibrated?

### Takeaway

Independent controlled Python subprocesses distinguish CPU work, sleep, and 64 MiB touched allocation. Existing wait4 helper records user+system CPU only; separate user and system CPU fields were not preserved by that helper, so those components cannot independently be calibrated from wait4 records.

### Cited findings

| Controlled run | wait4 wall / CPU / peak RSS | Independent `/usr/bin/time -l` real / user / sys / peak RSS |
|---|---|---|
| CPU | 0.532 s / 0.513 s / 12,550,144 B | 0.53 / 0.18 / 0.33 s / 12,550,144 B |
| Sleep | 0.632 s / 0.0125 s / 12,517,376 B | 0.63 / 0.00 / 0.00 s / 12,484,608 B |
| 64 MiB touch | 0.242 s / 0.0179 s / 79,593,472 B | 0.23 / 0.00 / 0.00 s / 79,609,856 B |

[CPU](evidence/native-memory-20261117-v1/calibration-cpu.json), [sleep](evidence/native-memory-20261117-v1/calibration-sleep.json), [memory](evidence/native-memory-20261117-v1/calibration-memory.json), [helper](evidence/native-memory-20261117-v1/calibration_helper.py). These are **separate invocations**, and `time` rounds CPU to hundredths; identical RSS is not required. CPU helper uses repeated process-time queries, explaining system CPU.

### Inferences

- Wall/CPU separation and memory scale agree within expected independent-run/rounding variation, not a proof of exact accounting for all process trees. macOS wait4 RSS is bytes and its descendant scope is OS-accounted, not a simultaneous process-tree maximum. [Existing owner](../../tests/performance/measure.py).

### Gaps

- Individual wait4 user/sys components were not recorded. No baseline/drain/sampler extra runs were performed.

## How was evidence collected and bounded?

### Takeaway

Pinned debug1 release hash `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5` and matching archived dSYM were used. Synthetic disposable fixtures only, disabled embeddings, clean HOME/TMPDIR and `PATH=/usr/bin:/bin`; network absence was intended, not independently traced. [Build receipt](evidence/build-archives-20261117-0046a6d-v1/symbolized_release/build.json); [one-off launchers](evidence/native-memory-20261117-v1/).

### Cited findings

- Corrected successful live capture executes exact product argv `letopis index --full` with `MallocStackLoggingNoCompact=1`, then `/usr/bin/leaks -noContent -fullStackHistory --outputGraph=<path> <pid>`. Offline: `/usr/bin/vmmap -summary <graph>`, `/usr/bin/heap -s -noContent <graph>`, `/usr/bin/malloc_history <graph> -allBySize -highWaterMark`, `/usr/bin/malloc_history <graph> -callTree -invert -ignoreThreads`, `/usr/bin/leaks -noContent <graph>`. Exact argv/environment and exit outcomes are in [launcher](evidence/native-memory-20261117-v1/nocompact-launch.py), [product receipt](evidence/native-memory-20261117-v1/nocompact-10k-index/product-receipt.json), and analysis receipts beside outputs. No memory contents requested.
- Coordinator resource ruling under user autonomy raised combined evidence soft threshold **2 → 4 GiB** subject to at least 20 GiB free; this was not an explicit user-set budget. Original scripts used a fixed 4 GiB guard and did not continuously check free space. Their [guard log](evidence/native-memory-20261117-v1/guards.jsonl) records 1,918,103,552 B before the rerun. Separately reported `df` observations of 61,488,824 KiB before and 59,160,812 KiB after, and final evidence size of 2,096,918,528 B, have no retained raw receipt and are not independently verified. CPU captures preserved.
- Only task-owned synthetic repository copies were used; existing fixture reset/oracle helpers copied idle projections, not active WAL. No product/source/Cargo edits or builds.

### Gaps

- Later report writer must not treat wrapper exit as product exit, instrumented snapshot as baseline RSS, or these partial results as full command coverage. Human-readable source paths and addresses are present in local evidence despite `-noContent`.
