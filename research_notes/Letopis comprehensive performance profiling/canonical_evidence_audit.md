# Canonical survey evidence audit

2026-11-17. **PASS with reporting/provenance qualifications**, not a claim of comprehensive profiling. Static parsing and hashing only: no product, build, benchmark, profiler, or test execution. Only completed 60/1k/10k evidence inspected; 100k in-progress evidence not opened. Existing reports and raw artifacts unchanged.

## Findings requiring owner attention

1. **FAIL — stale discrepancy claim, not current survey defect.** `baseline_interpretation.md:21` says the survey table reports 10k check/status/refresh_one RSS medians 43.88/40.44/49.61 MiB. The current `canonical_survey.md:69-71`, resource-summary JSON, and raw measured rows agree instead on **62.47/59.40/51.73 MiB**. Owner should remove or correct that stale sentence; no raw or survey-table correction is warranted.
2. **QUALIFICATION — provider activity is not directly observed.** `canonical-survey-10000-20261117-v1-launch.json` contains `provider_ruling: "... zero provider calls ..."`. Disabled fixture config and disabled-FTS rows support intended non-use, but do not establish an independently traced zero. `canonical_survey.md:107` correctly says network traffic was not traced. Treat the launch field as a ruling/expectation, not observed network evidence; annotate any downstream quotation.
3. **QUALIFICATION — actual per-call argv is not retained in raw rows.** `samples.jsonl` rows contain scenario names, not executed argv or stdin. Archived `harness/tests/performance/scenarios.py:97-127,160-172` maps `reindex` to `index --full` and refresh cases to `search` after one/two/all-file edits; archived `run.py:174` passes that scenario argv to the measurement owner. This supports the survey's workload interpretation, but is not an independent process-level argv trace. Runner launch JSON records the survey command, not every product command.

## Independent raw-row reconciliation

Recomputed from each scale's `samples.jsonl`, grouped by exact scenario and phase, using median/min/max of row values; RSS converted from recorded bytes by 1,048,576. Compared all 30 current survey table rows within displayed rounding and every median/min/max in all 45 phase-series entries of `evidence/canonical-survey-resource-summary-20261117-v1.json`: **zero discrepancies**. No derived summary was used as input to the calculations.

| 10k measured scenario | n | Wall median (min–max), s | CPU median, s | RSS median / max, bytes | RSS median / max, MiB |
|---|---:|---:|---:|---:|---:|
| check | 20 | 1.047962063 (1.019874292–1.182813667) | 1.019178 | 65,503,232 / 68,059,136 | 62.46875 / 64.90625 |
| status | 20 | 0.993107271 (0.959550584–1.103139583) | 0.9746105 | 62,283,776 / 65,896,448 | 59.3984375 / 62.84375 |
| reindex (`index --full` per archived recipe) | 3 | 4.627904083 (4.484957209–4.966385333) | 4.471358 | 80,986,112 / 81,182,720 | 77.234375 / 77.421875 |
| inspect | 3 | 1.372925292 (0.733593125–1.413919542) | 0.614514 | 20,021,248 / 20,037,632 | 19.09375 / 19.109375 |
| refresh_one (`search` after one-file edit per recipe) | 3 | 0.456478 (0.434827–0.468071), rounded | 0.412188 | 54,247,424 / 54,575,104 | 51.734375 / 52.046875 |

The reported RSS maximum is the maximum of recorded **individual wait4 lifetime peaks in that series**, not combined instantaneous RSS or a conditioning-phase maximum. Recorded CPU is wait4 user+system with OS-accounted reaped descendants; no raw split or independent OS calibration is present in these survey rows. All 352 rows report CPU seconds and RSS bytes; RSS is not allocation volume.

## Fixture, phases, and identity

- **PASS — counts and settings:** corpus manifests record seed 65, 2,000 bytes/fragment, 30/file, and 60/1,000/10,000 knowledge with 300/5,000/50,000 edges and matching source counts. Independent byte scans of retained fixture knowledge files find respectively 60/1,000/10,000 element markers, equal external declarations, and 240/4,000/40,000 internal declarations. This validates declaration counts, not independently every parsed graph relationship or source identity. Seed is recorded provenance, not independently reconstructible from hashes alone.
- **PASS — fixture bytes:** all 3/44/434 named source and knowledge input SHA-256 values match retained fixture files. Corpus fragment-size assertions are manifest evidence; this audit did not independently parse every fragment boundary.
- **PASS — phase accounting:** 123/123/106 rows, total 352; first-use 1/1/1, conditioning 10/10/10, measured 112/112/95. All rows say success, exit 0, and `validation.valid=true`; no recorded timeouts. Five cheap scenarios have two conditioning calls each, with 20 measured each except 10k inspect's three. Four mutation/full cases have three measured each. This audits recorded outcomes, not independently re-executed correctness or the supervisor's historical process state.
- **PASS — adaptive inspection:** 10k inspect conditioning raw wall times are 4.553130542 and 0.661967833 s, followed by three measured samples. Small n and shared-host variation preclude attributing their difference to a particular cache or operation.
- **PASS — build/file hashes:** current `HEAD`, stock and symbolized receipts all identify `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. All 116 pinned input hashes match both current product files and archived `source/` copies. Actual archived stock executable hashes to `77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`; symbolized executable to `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`; both match receipts. All three survey build receipts reference the stock hash. Original and archived build metadata copies hash to `6f915ede08d404679a9f9541d1b02ce5e8052a18080be60fcc37a0ba7f523bd7`.
- **PASS — retained raw output:** all untruncated stdout captures match their recorded SHA-256. Product input/receipt correspondence and post-build archive are not a cryptographic compiler attestation; individual sample rows do not themselves pin executable bytes.
- **PASS — cache and provider wording in survey:** first-use is an uninitialized-projection search, not evidence of an OS-cold cache; survey explicitly calls OS cache uncontrolled. All three retained fixture configs have embedding disabled, and no provider/model performance is established.

## Scope

No check here proves no remote requests, absence of omitted launches, contention-free host conditions, full semantic correctness, or historical build commands independently of their receipts. No heap/allocation profile, symbolized sampled matrix, 100k result, real provider, cold-cache measurement, or exhaustive CLI coverage is established by these rows.
