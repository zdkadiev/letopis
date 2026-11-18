# Canonical CPU evidence audit

**PASS, qualified:** saved CPU evidence supports the claims below. This is an offline audit, not a new performance run or validation of memory, provider, network, or every command.

## Independent checks

- Read `canonical_cpu_profiles.md` and the campaign's `completion.json`, `final-validation.json`, `analysis-summary.json`, `manifest.json`, raw captures, symbol sidecars, receipts, result validation, and pair records.
- Recomputed PID-filtered stack CPU and inclusive function weights directly from **all 12** compressed profiles and their presymbolicated sidecars. For every capture, stack count, CPU total, and the complete function-to-weight mapping equal `product-attribution.json`; profile and sidecar SHA-256 match that attribution. Every profile contains exactly one library entry matching archived executable path and code ID. This reproduces the existing owner's per-stack deduplication and CPU-delta weighting, not an independent sampling experiment.
- Archived executable SHA-256 independently matches `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`. `dwarfdump --uuid` independently reports `FFEBF8EC-A110-37A6-AA8B-747F67DAEA21` for both executable and dSYM. All **116/116** current build-inventory inputs match their saved SHA-256, including `src/service/document_refresher.rs`.
- Completion lists 12 captures and 10 pair files exist; each pair has two successful correctness-validation records. Pair receipt records equal actual case-file/config hashes; this is not proof that every aspect of the two process environments or cache state was identical.
- Complete 100k index receipt records sampler success/exit 0 and product JSON `ok: true`, revision 3, 3,334 files updated, zero invalid files. Separately saved result oracle records 100,000 knowledge nodes, 100,000 source nodes and 500,000 edges. This is a completed invocation, not a prefix. The 100k status child instead timed out after 10.013 s with exit -15: only its initialized prefix was sampled.
- Complete 100k index, product PID 94235: **183,971 usable stacks**, **177,888,334 µs** observed CPU. Independent sidecar-resolved, per-stack-deduplicated sums: `EffectiveGraph::apply` 94,906,805 µs / 177,888,334 = **53.35%**; `collect_source_references` 40,262,792 = **22.63%**; `reconcile_sources` 42,548,702 = **23.92%**. These are inclusive, overlapping CPU-presence percentages, not additive or wall-time shares.
- Matched source shows `publish_changes` iterating updated files and calling `graph.apply` at lines 384–395; `apply` invokes `self.edges.retain` at 949; `collect_source_references` loops over edges at 985 and scans knowledge keys via `any` at 989. Captured optimized source locations 948 and 1004 carry enclosing scope attribution, not proof of exclusive cost at those exact textual lines. Repeated scans are scaling hypotheses, not measured call counts or controlled wall-time savings.
- Three check pairs have wrapper/ordinary ratios **2.1521, 2.3802, 1.8415**, median **2.15×**; median paired difference **1.302544 s**. This is wrapper-inclusive end-to-end overhead on these 10k cases, including collection/saving, not intrinsic CPU slowdown or a 100k estimate. No same-build 100k ordinary control exists.

## Measured hotspot matrix

Percentages below are inclusive observed CPU presence; nested rows overlap. Single captures unless specified; sampling does not measure blocked time.

| Command / fixture | Observed sampled scopes | Qualification |
| --- | --- | --- |
| `index --full`, 100k | graph apply 53.35%; source reconciliation 23.92%; collect source references 22.63% | One complete sampled invocation; 183,971 stacks. |
| `index --full`, 10k | graph apply 19.81%; reconciliation 9.72%; collection 4.84%; SQLite source 33.58% | One complete sampled invocation; different fixture and provider configuration from 100k. |
| `check`, 10k | source observation 81.99%, 81.38%, 84.55%; root verification 55.73%, 54.72%, 57.19% | Three captures; inclusive scopes overlap. |
| `status`, 10k | source observation 85.92%; file retrieval 79.17% | One complete capture. 100k status is a censored prefix only: provenance file 84.31%, filesystem file 76.41%. |
| `inspect`, 10k | Git object write 62.40%; object sync 24.64%; `__fcntl` 22.17% | One capture; CPU profiles cannot assign remaining elapsed time to I/O wait. |
| `search`, 10k unchanged | change detection 72.96% | 111 stacks, one capture: coarse evidence only. |
| `traverse`, 10k unchanged | change detection 66.69% | 109 stacks, one capture: coarse evidence only. |
| `search` after 10k one-file refresh | collect source references 56.11% | 426 stacks; refresh is not a separate index command. |
| `search` after 10k bulk refresh | provenance 20.87%; graph apply 19.22%; projection publication 19.21% | One capture; overlapping scopes. |

No CPU hotspot claim here for `init`, `config`, or `drain`. Disabled provider configuration is not independent network tracing; provider performance is unmeasured. Tiny search/traverse captures do not support robust rankings, distributions, or tail estimates.

Evidence root: `evidence/canonical-cpu-20261117-v1/`; each named capture retains raw profile, sidecar and attribution. Audit executed only read-only parsing, hashing and `dwarfdump`; no builds, tests, product commands, samplers, memory instrumentation, or agents.
