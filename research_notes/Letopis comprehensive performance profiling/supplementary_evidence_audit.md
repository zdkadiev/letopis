# Supplementary catalog evidence audit

**PASS, qualified:** the saved catalog and selected measurements reconcile, with the real-snapshot and vector-canary limitations below. This is an offline audit, not a rerun, production-cleanliness certification, or canonical workload replacement.

## Independent accounting and identity

- Parsed `cases.json`, `manifest.json`, all `measurements.jsonl` rows and saved `plan-*.json`: **159 distinct cases**, 159 validation rows and 740 baseline rows. All 899 planned sample IDs match 899 unique rows exactly, with no missing, unexpected or duplicate IDs; planned and recorded campaign/stage/case/build/fixture/config/phase/repeat identities match. All rows are attempted, marked valid and successful, with no skips or timeouts. One validation and one baseline observation per case have exit **1**: `sparse-100.invalid-argv` intentionally expects `invalid_request`; the other 897 exit 0.
- Independently recomputed baseline repetitions: **13 cases × 1**, **129 × 3**, **17 × 20** = 740. Every observation is `repeat`; no added conditioning/first-observed samples. Each case's baseline rows have one campaign, stage, case hash, executable hash, build hash, fixture hash and config hash; each baseline's non-campaign validation-gate identity matches its validation row. Recomputed median/min/max and n for **all 159** series match `campaign-control/case-results.json`, without cross-identity pooling.
- Rehashed all **1,798 actual foreground stdout/stderr files**, checking byte counts too: zero mismatches. `cases.json` digest matches manifest. All 55 saved build-pin records match their archived executable, compiler metadata and full product-build receipt hashes. Every pin records source revision `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`; all measured rows use stock executable SHA-256 `77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db` and one build identity. These checks authenticate saved pins, not independently freeze the current working tree or prove every environmental factor equal.
- Every row's saved raw `ru_utime`, `ru_stime`, and Darwin `ru_maxrss` equals the recorded user/system seconds and RSS bytes. wait4 scope is direct child plus OS-accounted reaped descendants; RSS is a lifetime maximum, not simultaneous process-tree RSS. Block operations are not byte counts; byte I/O is unavailable.

## Representative independently recomputed series

Each entry is **median [min–max]** across the given n. Wall, user and system are seconds; RSS is MiB, converted from raw bytes using 1,048,576. These are separate marginal summaries; median user and system need not belong to the median-wall invocation. Foreground excludes setup, OS caches are uncontrolled, and n=3 or n=20 does not establish tail percentiles.

| Case | n | Wall s | User s | System s | Peak RSS MiB |
| --- | ---: | --- | --- | --- | --- |
| sparse-0 init fresh | 20 | .0662 [.0581–.1701] | .00471 [.00451–.00598] | .01054 [.00985–.01699] | 11.12 [10.98–11.30] |
| sparse-100 config | 20 | .0166 [.0114–.0673] | .00249 [.00228–.00322] | .00228 [.00183–.00436] | 8.58 [8.55–8.61] |
| sparse-100 status | 3 | .0199 [.0187–.0202] | .00406 [.00382–.00413] | .00441 [.00396–.00448] | 11.45 [11.38–11.62] |
| sparse-100 check | 3 | .0319 [.0315–.0326] | .00700 [.00695–.00701] | .01310 [.01273–.01416] | 12.62 [12.61–12.66] |
| sparse-100 inspect batch | 3 | .1019 [.0980–.1048] | .00808 [.00785–.00849] | .02336 [.02262–.02340] | 13.48 [13.48–13.77] |
| sparse-100 selective search | 3 | .0319 [.0312–.0373] | .00695 [.00659–.00699] | .01336 [.01321–.01340] | 12.81 [12.67–12.88] |
| sparse-100 traverse, both | 3 | .0308 [.0283–.0315] | .00697 [.00687–.00745] | .01325 [.01274–.01362] | 12.88 [12.86–12.91] |
| sparse-100 index --verify | 3 | .0299 [.0297–.0312] | .00626 [.00602–.00638] | .01268 [.01219–.01282] | 12.31 [12.23–12.36] |
| sparse-100 mock drain | 3 | .9948 [.9606–1.0663] | .06553 [.06179–.06589] | .09489 [.09448–.09529] | 13.64 [13.52–13.78] |
| sparse-100 mock populated search | 3 | .0471 [.0413–.0507] | .01230 [.00972–.01347] | .01989 [.01658–.02354] | 13.89 [13.70–14.02] |
| canary-2 vector-only | 3 | .0342 [.0317–.0763] | .00680 [.00675–.00686] | .01305 [.01254–.01314] | 13.27 [13.20–13.30] |
| long-1000 full index | 3 | .3565 [.3228–.3781] | .28290 [.26789–.29078] | .04079 [.03615–.04541] | 21.84 [21.17–23.59] |
| sparse-10000 full index | 3 | 1.1656 [1.1266–1.1805] | .91685 [.90216–.92234] | .15987 [.14765–.23146] | 34.39 [34.33–35.61] |
| real snapshot full index | 3 | .0803 [.0783–.0859] | .02616 [.02522–.02710] | .03521 [.03450–.03760] | 14.86 [14.72–14.88] |
| real snapshot check | 3 | .0561 [.0543–.0596] | .01324 [.01321–.01431] | .02886 [.02781–.03265] | 13.48 [13.34–13.50] |

## Real snapshot: actual observed load

Raw before/after metadata for `real.index-full` records **45 knowledge nodes and 204 edges**, revision 1 → 2; each JSON response reports **13 files updated, zero invalid files updated**. The saved `get_state` telemetry does **not** report a source-node count; source-node total is **unavailable**, not zero. The real fixture manifest's placeholder `knowledge: 0` is not the measured projection count.

Raw `real.check` and `real.status` outputs each contain **89 findings** in every baseline repeat: 44 `source_snapshot_unavailable` and 45 `source_not_found`. Preserved missing history and paths materially affect diagnostics. Valid execution means the harness accepted this real-fixture diagnostic scope; it does **not** mean a clean production knowledge base. The 0.0803 s full-index median is for this bounded exported snapshot and its 45-node/204-edge projection, not all repository data or a full production graph.

## Mock and canary qualifications

All four recorded mock-drain runs (one validation, three baseline) begin with 100 eligible queued hashes and no vectors; each ends with queue 0 and 100 vector keys. Each records 100 mock items/requests and a native observer result: **100/100 covered knowledge nodes, dimensions 32, 0 missing, eligible, exhausted, failed or invalid**, config identity `b75796da8c41b38136cd4f28d94ebc12c406f566d72d87d4ede4f473f9318d21`. The mock endpoint is not real-provider performance, and the mock server's resources are outside product wait4.

Each canary baseline receipt records one mock request, zero query errors and the expected returned ID. The disposable cache was removed: an independent raw FTS `MATCH` query is **unavailable**, not a passed test. Frozen input text and these receipts support the reported participation check only within that limitation.

The separate Python aggregation wrapper and its review fix are historical, unexecuted preparation; they provide no additional CPU result. No product, build, test, sampler, profiler or heap tool ran during this audit.
