# Native memory evidence audit

**PASS, qualified**, for the recorded instrumented runs and prefix-only allocation observations. Corrections and limits below are material; this is not a complete memory profile, proof of no leaks, or approval for further measurements. Audit read saved evidence and hashed files only; no product, build, test, sampler, or heap-tool commands ran.

## Completed extension: independent receipt checks

The three `native-memory-extension-20261117-v1/capture-*/receipt.json` records show direct product exit 0, successful result/count validations, and the following raw macOS wait4 `ru_maxrss` values in **bytes**. Captured stdout files independently hash to the receipt values and decode to successful product JSON. The existing `tests/performance/report.py` oracle checks command results and separately reads projection counts; the recorded counts are 10,000 knowledge, 10,000 source and 50,000 edges in every case. These are saved oracle results, not a rerun of the oracle.

| 10k command | Product PID | Raw wait4 peak | Result detail |
| --- | ---: | ---: | --- |
| `check` | 63065 | 71,991,296 B | Empty errors |
| `inspect` | 63268 | 24,002,560 B | Expected ID, snapshot, location and hash recorded |
| `index --full` | 64543 | 85,082,112 B | Revision 3; product JSON reports 334 files updated and zero invalid |

Raw and formatted wait4 peak values agree. All are below 1 GiB; launcher checks missing/exceeding RSS **after** completion and before subsequent cases, not continuously. These instrumented lifetime peaks include OS-accounted reaped descendants, not simultaneous tree RSS or uninstrumented baselines. The earlier direct live launchers did not save wait4 peak: their requested post-observation guard cannot be declared passed retroactively.

For each new capture, receipt PID, `leaks` target PID, and native report PID agree; saved `ps` output identifies the private executable and parent 62939. The launcher checks this immediately before attach, but a check-to-attach race remains. Independently hashed private executable and all four copied build artifacts, including dSYM files, match archived build-receipt SHA-256; executable hash is `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`.

## Snapshot and allocation claims

Native report launch-to-snapshot timestamp differences are **0.187 s check**, **0.133 s inspect**, **0.738 s index**. This is not time from input EOF. Reported high-water / current allocated **heap plus VM**, respectively, are 26.9 / 6.5 MB, 4.4 / 4.0 MB and 30.7 / 30.6 MB. These logged prefix high-water values are neither RSS nor full-command allocator high-water; later allocations and lifetime churn are unobserved.

- New check high-water report lines 28–30 show 7,864,320 B on `edges_get_by_source_id` and 5,767,168 B on `nodes_get`, both under `GraphDiagnosticsService::find_problems`.
- New index report lines 28–30 show 9,960 calls / 8,047,680 B under `BasisPreflight::add_file` BTree construction and 4,166 calls / 5,565,776 B under `EffectiveGraph::from_storage` BTree construction. They are allocation-stack origins at the logged prefix high-water, not proof of later ownership or a controlled optimization result.
- New inspect report lines 28–32 support 2,146,304 B on signal-listener thread creation, 559,104 B SQLite page allocation and 379,968 B Git object-write compression initialization. VM reservation is not necessarily resident.
- Original early index report lines 12–32 shows a **0.026 s** snapshot, 12.5 MB logged prefix high-water, 52.9 MB cumulative prefix allocations, 240,074 allocation and 189,551 deallocation operations. It reports 5,767,168 B `nodes_get` vector growth and 1,192,464 B / 273 calls SQLite `pcache1Alloc` under `EffectiveGraph::from_storage`. No full-lifetime allocation or peak inference follows.
- Offline leak reports for the original and all three extension snapshots each report zero leaks and zero leaked bytes; corresponding extension tool receipts report exit 0. This only describes the scanned snapshots, not future leaks or reachable retention. Original `live.txt` explicitly reports missing earlier allocation stacks: 480 malloc blocks and 5 VM regions without stacks. `-noContent` does not eliminate source paths or addresses in reports.

## Calibration and resource ruling

Saved CPU/sleep/64 MiB calibration receipts show wait4 and `/usr/bin/time -l` were **separate invocations**, not simultaneous measurements. The reported wall, CPU and RSS numbers match those receipts; wait4 stores combined user+system CPU in the original calibration, and `/usr/bin/time` rounds its separate user/system fields. Both record OS block operations, not per-process I/O bytes. Agreement is qualitative calibration, not proof of exact tree accounting.

The 4 GiB combined-evidence soft cap was a **coordinator resource ruling under user autonomy**, not a user-explicit budget. Extension `manifest.json` records this provenance; launcher applies 4 GiB only while observed free space is at least 20 GiB, otherwise 2 GiB. Extension guard log starts at 2,164,133,888 B allocated and 62,580,785,152 B free; all recorded checkpoints remain under their effective 4 GiB cap. Original direct-launch scripts have fixed 4 GiB checks and do not themselves implement a continuous free-space guard; the note cites separate before/after free-space observations. Neither phase had a hard instantaneous RSS or disk limit.

## Reporting corrections

1. `memory_results.md:22` says final extension evidence/free values are **2,498,420,736 / 62,239,973,376 B**, but the linked `guards.jsonl` ends at **2,498,408,448 / 62,242,656,256 B**, before the final offline leak analysis. The stated later figures may be separate observations, but no corresponding raw record is linked; do not treat them as independently verified from that log.
2. `memory_results.md:44` says no check or inspect heap captures; read it as describing the **original phase only**, superseded by the three extension captures in lines 3–22. Status, traverse and search remain without valid heap captures in this evidence.
3. The original successful index and extension captures remain distinct runs. Do not transfer the extension's wait4 guard result to the original, infer complete-command allocator peaks from snapshots, or present the 4 GiB ruling as explicitly requested by the user.
