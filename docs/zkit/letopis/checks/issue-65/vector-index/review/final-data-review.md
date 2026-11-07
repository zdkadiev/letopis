# Independent final exact-index data audit

2026-11-07. Read-only audit except this report. No builds, suites, provider calls,
SQL/database probes, benchmark/campaign calls, or307MB payload scans were run.
This report follows the separate code review and measurement-method preflight.

## Verdict

No material data discrepancy or missing proof found in the completed artifacts.
The correct wall-time conclusion is **no established full-CLI improvement**:
before median65.5928157500457s, after65.82987516606227s, ratio0.996398908316041,
descriptive latency reduction **-0.3614106412505924%**. These three-pair shared-host
results must not be presented as a CLI speedup. The earlier approximately20.03x
native SQL result remains a separate diagnostic workload/result.

## Ordinary comparison

Independently checked all six raw/final records, their manifests and saved native
mode setup receipts. Each raw record equals its final record after removing the
validation object. All calls exited0 with success, ok:true, no warnings, empty
stderr, complete untruncated captures, and exactly the five independently expected
IDs with clean flags and revision5. Recomputed every capture length and SHA-256.
All six complete stdout byte sequences are identical:
`0049eca6e5b11edf62ed5bf46ba40b1f89be69d48b3b408856ccf56589da85a2`.
Thus result order, paths, coordinates, revision and flags match across modes,
in addition to exact literal membership.

The six monotonic intervals do not overlap and occur in the promised order:
0-before,0-after,1-after,1-before,2-before,2-after. Every saved mode setup succeeded,
has empty stderr, and its final native descriptor matches the subsequent manifest's
none/cos or flat/cos. Per-call manifests select the intended pinned executable,
request case000000/limit5, enabled real provider,100000 vector keys, zero failed
embedding rows and the same embedding identity. The preflight-approved drivers'
SHA-256 values match comparison-manifest.json; setup/warmup/help/conditioning and
profile are excluded from the six summary records.

Recomputed summary arrays and all statistics directly from per-call records:

| Metric | Before | After |
| --- | --- | --- |
| Seconds |75.46495524980128,65.20979791600257,65.5928157500457|65.82987516606227,66.98608800000511,62.75632154196501|
| Median seconds |65.5928157500457|65.82987516606227|
| Median CLI CPU seconds |61.260561|60.878424|
| Maximum recorded peak RSS bytes |197066752|166641664|
| Maximum recorded peak RSS MiB |187.9375|158.921875|

Before RSS samples are [189497344,189775872,197066752]; after are
[166641664,161726464,162938880]. CPU samples are
[68.341896,61.260561,60.412094999999994] and
[60.878424,61.85576400000001,58.90912]. RSS uses MiB=2^20 bytes and the recorded
wait4 lifetime maximum scope; it excludes the separate Ollama daemon and is not
simultaneous whole-stack memory. The lower recorded CLI RSS is descriptive for
this campaign, not a production memory guarantee.

## Source/binary identity and actual-root checks

Independently rehashed the before binary and its106 captured source inputs, and
the after binary and all107 actual-root inputs including storage/vector_index.rs.
Rehashed those same107 inputs in the isolated after capture; all match. Verified
the isolated capture's HEAD and the unchanged actual-root HEAD separately:

- Before capture2db40fd82fd8b504ebb2040b96b248031ce5d791; binary SHA-256
  `bd2c79b6b3dff6858fd021ca7af521f9ceff735279844d76638f1879fa9fb0b6`.
- After capture016eae8fa9705a209b01555a32877702b70cf74a; binary SHA-256
  `ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9`.
- Actual root HEAD52ed491a6688e0a4eee0cb5eceb3fc19f900465e remains unchanged;
  intended product edits are uncommitted and byte-identical to the isolated
  after capture. The capture commit is not claimed as actual root HEAD.

Host storage manifest is PASS with16 listed commands, each exit0 and an existing
log, on macOS arm64. Its recorded inputs include all107 matching product inputs.
Rehashed the nested actual-root make-verify log to
`693344304c25f919da84774a42e19d2c655ee32fd538b8f84d2208258cb5c751` and parsed
all171 successful test-result groups:2443 passed executions,22 ignored, zero
failed, final `ok`. These are execution counts across profiles/examples, not a
unique-test count. The host manifest also records schema parity, native capability,
runner checks, strict documentation, copied standalone execution and linkage
checks. This audit did not rerun them and does not imply a Linux/x86 result.

## First product opening and payload evidence

The first actual product writable opening is saved separately:3.4351550831925124s,
success/exit0 and an ok:true init response. Its scope includes ordinary init plus
checked migration and is not pooled into steady-state search timings.

The saved native coverage receipt after product migration reports100000 native
vectors and100000 keys, dimension768, and zero wrong widths, absent/orphan
mappings, missing knowledge vectors, recorded errors or reserved attempts.
The raw product-upgrade-vector-hash.txt reports100000 rows and307200000 vector
payload bytes, with the original ordered SHA3-256:
`4ED319020B3ADFE2FD6FC375EE308B6266F2D9E74689A6DE1EF7E22488218EE4`.
This agrees with the separately audited native baseline payload receipt; the
accepted validation identifies the exact baseline SQL source.

The first wrapper's rejected hash attempt is preserved separately. Its query
spelling differs (`rowid,vector` versus `rowid, vector`); it is explicitly not
accepted as payload-change evidence. The accepted rerun uses the exact baseline
SQL and matches its hash/counts. No payload-loss conclusion is justified by that
rejected wrapper assertion. This reviewer checked the saved raw/validation data,
not a new native scan. The coordinator owns the final4334-file/original-fixture/
historical-artifact preservation rehash; it is not duplicated by this audit.

## Genuine profile and numerical attribution

Validated compressed profile SHA-256
`bc7a3c5166b7c1ef03257e2d597f2ac1900b574bbbb5607ee040b738cf983125`, size853519
bytes, two threads,79244 samples and79244 usable stacks. Resolved raw stack
addresses with the saved symbol sidecar and independently recomputed **all1646
function CPU totals/observation counts**, not only selected displayed rows.
They match stacks.json; the complete observed CPU delta is **70303137us**.
The earlier approximate main-thread-only number70303078 excludes59us on the
other thread and should not replace that complete total.

`dwarfdump --uuid` on the pinned final binary reports
`72CF737B-8ADA-393A-AC42-37E4C4EF088C`, matching the profile's letopis library and
sidecar debug/code IDs. The receipt records the genuine Samply save-only1000Hz
command, correct final binary hash, success and the same hybrid stdout. It is
explicitly profile-only, recorded after ordinary pairs; wrapper resources and
86.55962629104033s duration are not an additional ordinary CLI sample.

Recomputed selected inclusive CPU shares against70303137us:

| Scope | CPU delta us | Inclusive share |
| --- | ---: | ---: |
| eligibility |67986422|96.70467763%|
| historical source hash |25189645|35.83004411%|
| CurrentKnowledge build_link |24435054|34.75670510%|
| current source hash |11367247|16.16890438%|
| Markdown file_parse |790794|1.12483459%|
| native vector retrieval |699803|0.99540793%|
| layout_scan |237590|0.33795078%|

These scopes nest and cannot be summed, and sampled CPU is not wall/blocked-time
accounting. Native text_index_get_by_expression and fts5Bm25Function frames are
also observed, separately corroborating FTS execution; result membership alone
would only establish lexical-hit retention plus vector contribution. Different
profile contexts do not establish paired sub-operation regressions or gains.

## Reporting boundaries

The corpus is synthetic with real-provider vectors. The native diagnostic uses
a stored vector and diagnostic SQLite3.54.1, whereas the complete product uses
SQLite3.53.2 and includes provider/refresh/provenance/filter/fusion work. Native
approximately20.03x is not a whole-product latency multiplier. First-open cost,
native SQL, ordinary CLI pairs and Samply are separate populations.

Per-call native rebuilding conditions OS cache/layout outside timers; this is
warm steady-state evidence with shared/uncontrolled host conditions. Three pairs
do not establish p95/p99, minimum-hardware fit or production acceptance. The full
CLI medians show no established improvement; the retained implementation may be
described as enabling the exact physical index with preserved outputs/bytes,
not as a demonstrated end-to-end wall-time optimization on this fixture.
