# Independent final data audit

**Verdict: PASS for the recorded local experiment and host verification evidence.** No Critical or Important data/reporting finding remains. This audit covers retained data, arithmetic, identities and methodology; it does not re-review the product code or rerun builds/tests.

## Independently recomputed measurements

| Limit | Ordinary n per side | Before median, seconds | After median, seconds | Ratio of medians | Latency reduction |
| --- | --- | --- | --- | --- | --- |
| 5 | 3 | 66.6220504580997 | 1.3435168750584126 | 49.58780324601665 | 97.98337507503858% |
| 100 | 3 | 70.9150112499483 | 1.866692375158891 | 37.98966138912528 | 97.36770488750327% |

Recomputed from each `pairs/*/measurement.json`, without using validation summaries as the numeric source. Each `validation.json` agrees with its raw wall time, wait4 CPU and RSS. Recomputed per-side CPU medians and maximum CLI peak RSS also match `comparison-summary.json`. Percentage recomputation agrees within ordinary binary floating-point rounding (about 1e-14 percentage points).

All 16 ordinary/screening/smoke receipts report successful zero-exit completion. Independently decoded their captured stdout bytes, checked byte length, truncation=false, SHA-256 and JSON, and compared exact ordered IDs against the appropriate independent before/bounded SQL oracle. The 12 ordinary outputs are byte-identical within each limit across both implementations. The omitted-limit and explicit-1000 smoke outputs equal the limit-100 output hash. This equality is true for this fixture/query and is not a claim that bounded search preserves full-corpus ranking generally.

The 12 ordinary samples occur in the prescribed order: before/after, after/before, before/after for L=5, then the same order for L=100. No ordinary sample was omitted. Retained screenings (L5 3.459733125 seconds; L100 1.451485333 seconds), the 144.235049291-second conditioning run, model warmup and both smoke calls are excluded from paired medians as planned. Samply is a separate later run.

## Source, binary and method identity

Independently hashed the current files against all recorded pins:

- 107 candidate product inputs: all match `product/pin.json`.
- 269 actual-root host-runner inputs: all match `host-storage/manifest.json`.
- 107 captured baseline inputs: all match `pairs/baseline-source-pin.json` in the older captured source tree.
- 27 method dependencies/artifacts: all match `pairs/method-pins.json`, including measurement helpers, scripts, SQL/JSON oracles, SQLite executable, native extension, provider/freshness receipts and corpus manifest.
- 4,334 canonical fixture files: all match the independently retained corpus manifest.

The candidate binary SHA-256 is `117bee3d746af4946c0a3a52f2aa6e8bd972e06ba25d2e982b1fe27a73af53ba`. The baseline remains the captured `ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9` binary. Dirty root source is explicitly represented by hashes; the root HEAD is not misrepresented as containing these changes.

## Earlier methodology gaps resolved within the stated scope

- Reservation is recorded in `pairs/reservation.json`: exclusive output creation plus a nonblocking campaign flock. The driver checks matching tool processes before each invocation and all retained validations contain an empty list. The orchestrator's declared no-owned-work overlap is consistent with these receipts. This is not prevention or proof of arbitrary external machine activity, and the report must retain that limit.
- Driver state includes config bytes, dimensions=768, native index configuration/model bytes, vector identity/count, query errors, embedding failures, graph counts and revision. Product, method and canonical corpus pins are checked at campaign boundaries; model digest is checked throughout. Provider evidence identifies Ollama 0.40.15 and resolved Gemma digest `101341d65c2ccbf23f16650b79d30b9fca94a45ffa09a9984c600157b81a58df`.
- The mirrored request includes `truncate:false`. Independently packing the retained oracle query vector as f32 reproduces SHA-256 `471f33abdda366f4d979e2b04071dc4e29acd22c144c2a34dc4afc9a51b46c92`, matching provider/warmup evidence.
- Independently rechecked the 1,000 source files against a Git archive of the declared basis and recomputed all 101 selected knowledge-range hashes, anchors, source bindings and source-range hashes through read-only SQL and raw bytes. They agree with `freshness.json`. Its 101 identities exactly cover the union of expected outputs and the largest bounded SQL pool. This establishes clean selected bindings for this unchanged synthetic fixture, not an independent full-corpus metadata audit or concurrent-edit guarantee.
- The earlier oracle script's broad 'independently clean corpus' wording should be read through the narrower, explicit `freshness.json` scope. Both baseline and candidate final selected outputs are covered. Independent ranking SQL produces 6/101 unique bounded candidates at L5/L100; actual product observation bounds are established by implementation/tests, not by these measurement receipts alone.

The final summary file is still written before the driver's final pin assertions, as noted in the method review. This does not invalidate this completed campaign: this audit independently rechecked every relevant final pin and corpus file successfully. Future reuse should require a terminal success record or repeat these checks rather than accepting summary-file existence alone.

## Host verification evidence

`host-storage/manifest.json` is PASS, with 16 recorded commands and exit code 0 for each, including actual-root `make verify V=1`, native capability checks and copied standalone execution/linkage. The rust log SHA-256 matches `rust-verification.json`. Independent parsing finds 170 successful test-result groups, 2,460 passed executions, zero failed executions and 22 ignored executions. These are logged executions across repeated/debug/release groups, not 2,460 unique tests. Linux was not run and is not certified.

## Genuine Samply identity and recomputation

The retained receipt invokes `samply record --save-only --rate 1000 --unstable-presymbolicate` against the pinned candidate executable. Its stdout independently matches the L100 oracle. The compressed profile hash matches its validation receipt. Actual `dwarfdump --uuid`, profile library code ID and symbol sidecar all agree on `519E4889-7285-3915-98A8-59BE598FA9D1`; binary SHA-256 also matches the product pin.

Independently traversed raw profile stack/frame tables and the symbol sidecar, deduplicating repeated function names within each sample stack. All **705** reported function inclusive CPU-delta totals and stack-observation counts match `profile/stacks.json`, including unresolved address labels. The profile contains 3,323 usable recorded stack rows across two threads, and summed observed thread CPU delta is 1,537,790 microseconds.

| Owner | Inclusive observed CPU, microseconds | Share of observed thread CPU |
| --- | --- | --- |
| Search eligibility (`get_eligible_links_by_metadata_and_filters`) | 117,272 | 7.6260087528% |
| Document refresh (`Refresher::refresh`) | 689,639 | 44.8461103272% |
| Vector retrieval (`get_nearest_vectors`) | 573,023 | 37.2627601948% |

These are inclusive, potentially overlapping sampled CPU costs, not additive phase durations or wall-time percentages. 'Search eligibility' is the precise first label; it includes source-binding/freshness/provenance work and should not be described as a pure snapshot-only measurement. Shared/inlined lower helper symbols can inherit broad attribution. No further bottleneck claim is needed to support the measured change.

## Limits and remaining parent work

The defensible performance statement is a warm local before/after comparison on this synthetic 100k-knowledge/100k-vector corpus, one query, real local Gemma inference, n=3 per side per limit. It is not production, minimum-hardware, cold-cache, cross-platform, tail-latency or confidence-interval acceptance. CPU/RSS exclude the independent Ollama daemon. SQL output bounds do not imply bounded SQLite/vec1 internal scanning or memory.

Original vector-payload preservation, final source-linked knowledge publication/readback and final documentation archive are parent follow-up items and were not certified here. No new product edits, builds or tests were performed by this reviewer.
