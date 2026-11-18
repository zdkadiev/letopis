# Drain-scale results

## Completed v2 campaign — 2026-11-17

After the coordinator revoked the operational UTC cutoff, the reviewed v2 launcher ran under exclusive ownership. **Three validations and nine ordinary baseline samples passed; zero skipped, failed or timed out.** Launcher elapsed wall time was **54.06 s**; this includes setup, validation, observer calls and launcher overhead, not just measured drains. Ownership is released.

| Unique texts | Validation | Baselines | Drain wall seconds: min / median / max | Product user seconds: min / median / max | Product system seconds: min / median / max | Peak RSS MiB: min / median / max |
|---:|---:|---:|---:|---:|---:|
| 2 | 1/1 | 3/3 | 0.047056 / 0.051378 / 0.054496 | 0.004919 / 0.004962 / 0.005120 | 0.007873 / 0.008008 / 0.008604 | 12.219 / 12.313 / 12.328 |
| 100 | 1/1 | 3/3 | 0.932611 / 0.997387 / 1.013247 | 0.060186 / 0.067246 / 0.070916 | 0.093459 / 0.107150 / 0.108570 | 13.438 / 13.656 / 13.797 |
| 1000 | 1/1 | 3/3 | 9.013907 / 9.040224 / 9.132058 | 0.840656 / 0.855949 / 0.897499 | 0.754424 / 0.758505 / 0.805436 | 14.578 / 14.672 / 14.859 |

The three baseline minor-fault counts were respectively **1056–1061**, **1334–1352**, and **3207–3222**. Major faults, reported input-block and output-block operations were zero in every baseline; zero OS block counters do not establish zero bytes of I/O. Setup, excluded from the measured drain interval, took **0.172–0.462 s**, **0.187–0.472 s**, and **0.246–0.576 s** respectively; individual setup process receipts are under each sample's receipt directory. Observer work also occurs outside the measured drain.

Every validation and baseline observed N distinct hashes, queue N → 0, N provider items, no recorded failures, and native current-configuration **N covered, zero missing/eligible/invalid, populated, 32 dimensions**. Provider HTTP request counts happened to equal N in these runs; correctness depends on item counts, not a request-per-item assumption.

Raw evidence: [all 12 measurements and raw resource counters](evidence/drain-scale-20261117-run-v2/measurements.jsonl), [per-sample setup/native/stream receipts](evidence/drain-scale-20261117-run-v2/receipts/), [completion marker](evidence/drain-scale-20261117-run-v2/adjunct-execution-complete.json), [launcher timing](evidence/drain-scale-20261117-run-v2/launcher-execution.log), [preflight](evidence/drain-scale-20261117-run-v2/preflight.txt), and [producer/build/observer/fixture pins](evidence/drain-scale-20261117-run-v2/adjunct-pin.json). Driver stage plans, accounting and argv/output logs are in the same run directory. Product revision `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`; the stock build's complete 116-input receipt and archived native observer were verified by the launcher.

Per-product cap was 120 s; stage budget was 1,800 s per invocation, not a hard 600 s global deadline. The observed peak combined scratch/evidence footprint recorded in rows was 100,313,791 bytes; free space after execution was 56,355,640 KiB. Driver disk guard and post-observation 1 GiB product/setup RSS stop are soft, not live hard limits. RSS is wait4 lifetime maximum with OS-accounted reaped descendants, not simultaneous tree peak or allocation data. Measurements use a local deterministic 32-dimensional mock, three repeats per scale, uncontrolled OS caches; they are not real model, network, tail-percentile or semantic-quality results. Search remains postponed.

## Earlier unstarted attempts — historical

## 2026-11-17 v2 authorization and cutoff

The v2 runtime received exclusive authorization at approximately 19:57 UTC, with a strict prohibition on starting any new Rust call after 20:00 UTC. **No v2 execution was started.** The reviewed launcher invokes all six validation/measurement stages in a single `--execute` call and has no absolute UTC cutoff; each stage independently permits 1,800 seconds and each product call up to 120 seconds. Starting it with less than three minutes remaining could therefore launch a later setup, measured command or native observer after the deadline. The post-stage resource guard does not enforce the UTC cutoff. No safe deadline-aware interruption/cleanup mechanism is provided by this launcher, and its reviewed source and identities were not modified.

The independently prepared [v2 campaign](evidence/drain-scale-20261117-run-v2/) and its [preflight](evidence/drain-scale-20261117-run-v2/preflight.txt) remain intact. All three validations and nine repeats are unstarted; no runtime statistics or native coverage claims are available. No product, observer, profiler, or build was run. Exclusive ownership is released. The v1 record below remains historical.

**No measurements executed.** Ownership transferred for this work and is now released; no product workload, observer, build or sampler was launched. Search remains postponed.

## Preparation and preflight

The reviewed launcher ran only its fixture preparation phase:

```sh
/opt/homebrew/bin/python3.12 -B \
  'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v1/launcher.py' \
  --prepare --evidence \
  '/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-run-v1'
```

Exit status zero. Preparation elapsed time was not recorded; no aggregate runtime claim is made. The new [run directory](evidence/drain-scale-20261117-run-v1/) contains the frozen selected catalog, manifest, producer/build/observer pins, preserved initial generated catalog and manifest, and prepare log. Its [preflight output](evidence/drain-scale-20261117-run-v1/preflight.txt) records matching current source and complete build/observer provenance, launcher and producer/selector/harness/infrastructure hashes, selected case hashes, and all **11 fixture inventories / 432 files**. Combined owned scratch and evidence: 12,221,652 bytes; available space before preparation: 57,328,068 KiB.

## Stop reason: cross-stage resource stop not enforced

The user required a **global** stop on an observed resource threshold. The frozen driver detects a successful sample with `resource_stop_reason` and skips later slots within that one CLI invocation (`tools/profile.py:264-269`), but its `resource_stop` state is reset on the next invocation (`:219-221`). The reviewed adjunct invokes a separate CLI stage for each validation and measurement (`launcher.py:178-187`). Its `verify_row` (`:65-75`) checks validity and counts but does **not** reject a nonempty `resource_stop_reason`. Therefore, if a successful final slot has product RSS ≥1 GiB, the launcher can begin a later stage despite the required global stop. A soft disk stop detected only post-sample has the same cross-invocation risk.

This conditional gap is not proof that any resource threshold was reached. It is an inability to guarantee the requested stop behavior before executing the reviewed launcher. The launcher and original driver were not modified or patched; no execution was attempted. The driver's 2 GiB scratch-plus-current-evidence guard remains stricter than the requested outer 4 GiB for those same roots, but neither is a live limit. Its 1,800-second budget is per stage, not a global 600-second cap; the requested 600 seconds is an aim only.

## Planned outcomes

No plan rows were submitted to the driver. They are listed here as **unstarted**, not skipped or failed; no timing, RSS, CPU or coverage statistics are available.

| Case | Validation | Ordinary repeats | Result |
|---|---:|---:|---|
| `sparse-2.mock-drain` | 0/1 | 0/3 | Unstarted |
| `sparse-100.mock-drain` | 0/1 | 0/3 | Unstarted |
| `sparse-1000.mock-drain` | 0/1 | 0/3 | Unstarted |

Next step: review a change to the adjunct that propagates an observed resource stop across CLI invocations, including successful or last-slot records, and records remaining planned outcomes; then repeat the static gate before starting product work. Do not interpret driver `is_valid` alone as satisfying the adjunct's extra count or stop requirements.
