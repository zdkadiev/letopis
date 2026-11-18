# Drain-scale v2 evidence audit

**PASS, qualified.** The completed run contains 12 distinct successful rows: three validations and nine baseline samples. This audit read saved receipts, recomputed descriptive statistics and hashed saved files; it ran no tests, product, observer, benchmark or build.

## Results and independent checks

| Unique texts | Validation / baseline | Baseline wall min / median / max, seconds |
|---:|---:|---:|
| 2 | 1 / 3 | 0.047056 / 0.051378 / 0.054496 |
| 100 | 1 / 3 | 0.932611 / 0.997387 / 1.013247 |
| 1000 | 1 / 3 | 9.013907 / 9.040224 / 9.132058 |

All 12 rows have distinct sample IDs, product exit 0, attempted/valid and not timed out. Each has N knowledge nodes and unique text hashes, queue N→0, N provider **items**, zero final failures, and a matching saved native observation with N covered/knowledge, zero missing/eligible/invalid, populated verdict and 32 dimensions. Six saved accounting files agree: three planned/completed validation slots and nine planned/completed baseline slots, no missing, duplicate, invalid, censored or skipped slots. [Rows](evidence/drain-scale-20261117-run-v2/measurements.jsonl), [receipts](evidence/drain-scale-20261117-run-v2/receipts/), [completion](evidence/drain-scale-20261117-run-v2/adjunct-execution-complete.json).

All 24 foreground stdout/stderr files match their recorded byte counts and SHA-256; no stream was truncated. Raw `ru_utime`, `ru_stime` and macOS `ru_maxrss` agree with displayed user/system seconds and RSS bytes in all 12 rows. Baseline min/median/max values in [results](drain_scale_results.md) agree with the saved rows. RSS is wait4 lifetime peak with OS-accounted reaped descendants, not simultaneous process-tree peak; zero OS block counters do not establish zero I/O bytes.

The campaign manifest and selected casebook match their pinned hashes. The v2 launcher, driver, historical casebook producer and proposal, selection, stock binary, complete build receipt, metadata, observer and observer receipt match recorded pins. Row binary and fixture hashes match campaign pins. Saved preflight reports source/build, observer and all 11 fixture inventories verified; this audit did not rerun those runtime gates. Product revision in the campaign pin is `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. The new 100-text case has distinct fixture/config identity from the older catalog cohort; **do not pool them**.

## Limits and interpretation

- Maximum recorded product RSS across the 12 rows: **15,581,184 bytes**; all 24 saved init/index setup receipts exited 0, maximum setup RSS **15,843,328 bytes**, no setup RSS stop flags. The v2 post-stage global stop code was active; no observed threshold breach occurred. No hard or instantaneous RSS limit is established.
- Maximum `disk_bytes_post` in rows is **100,313,791 bytes**, the driver's combined owned scratch plus current campaign evidence logical-file-size scope, not all task evidence or filesystem usage. The results note's post-run free-space number is not independently established by the linked launcher timing or preflight records.
- Launcher log records **54.06 s** for the whole execution, including setup, validation and observer work; it is not summed drain latency. Individual product cap was 120 seconds and driver budget 1,800 seconds per stage, not one global stage budget.
- The coordinator revoked the absolute UTC cutoff as an operational target, not a user deadline or security control. Earlier unstarted preflights in the historical section of the results are preserved history, not failed Rust samples and not the completed run.
- Three repeats per scale, deterministic loopback mock and uncontrolled caches do not support real-provider, tail-percentile or model-quality conclusions. Search remains postponed.
