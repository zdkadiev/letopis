# Independent review of bounded-search measurement method

Scope: `benchmark.py`, `oracle.py`, `oracle-before.py`, `condition.py`, `fixture-copy.json`, retained SQL/JSON oracle outputs, shared `tests/performance/measure.py`, fixture provider config, and the captured baseline build manifest. No ordinary series was executed by this reviewer. No product changes or code re-review were performed.

## Verdict

The paired experiment and ranking validation are sound for this single synthetic, already-vectorized 100k-knowledge/100k-vector workload, subject to the evidence gaps below. No timing arithmetic or loop-order bug was found. Do not yet label freshness independently validated or exclusive campaign execution proved solely by these scripts.

## Important evidence gaps before claiming acceptance

1. **Freshness oracle claim is not implemented in these oracle scripts.** `oracle.py:28` and `oracle-before.py` label the corpus independently clean; `benchmark.py:40` only asserts that product-reported flags are empty. SQL rank derivation does not establish canonical knowledge hashes, source bindings, snapshots, missing sources or fresh filesystem bytes. `fixture-copy.json` records database integrity and sizes, not an independent freshness check or the canonical file manifest. Link the existing corpus generator/manifest validation and verify the owned copy against it, or explicitly narrow the report to independent IDs/order plus product-reported clean flags. Output flags must not be described as their own independent oracle. This does not require repeating the product algorithm in Python.

2. **Exclusive campaign reservation currently exists only as a claim.** `benchmark.py:24` declares no owned build/test/drain/profile overlap, but the script imports only `measure_process` and acquires no campaign reservation. An externally owned reservation with a retained preflight/termination record is sufficient; otherwise reserve the campaign and record all owned jobs settled before the first screening. Per-process cleanup is already implemented correctly by the shared measurement helper. Do not run the review's own expensive checks concurrently with the timing window.

3. **Freeze fixture configuration and measurement dependencies.** `benchmark.py:15-23,41,59` checks binary hashes, product source inputs, counts, revision and provider identity, but not config bytes, query-error state, dimensions/index configuration, canonical corpus files, timing helpers, oracle outputs, SQLite executable or extension. At least record and recheck config and corpus-manifest identity at campaign boundaries, record vector dimensions/index configuration, and hash scripts/helpers/oracle artifacts and SQLite extension in the campaign manifest. A tag or count alone does not pin bytes or the provider endpoint. Do not hash mutable SQLite files naively across normal WAL activity; use logical state or an owned snapshot. The existing immutable baseline binary was independently SHA-256 checked against `product/after-build.json` and matches `ccf4a70e...`.

## Minor method improvements

- `oracle.py:5` hardcodes the configured endpoint and omits the product request's `truncate:false`. This particular short query is unaffected by truncation, and the current endpoint matches the fixture config. Mirror and record the exact payload and endpoint to avoid a latent mismatch; record the resolved Ollama model digest/version because a model tag alone can change. The JSON values are packed as 768 little-endian f32 values, consistent with this macOS arm64 product path.
- `oracle.py:10` uses a plain ATTACH filename: `sqlite3 -readonly` guarantees the main connection is read-only, not every attached database. The scripts issue only reads and use the owned copy, so there is no original-fixture risk; explicit read-only URI attachment would make the stated access mode exact.
- `benchmark.py:58` writes the summary before its final source-hash assertion. Consider validating final pins before publishing the aggregate, or stamp explicit campaign success/failure. A failed final assertion must invalidate the aggregate rather than leave it appearing accepted.
- Use the already working Python interpreter (>=3.10): `conditioning.log` records that the system interpreter failed importing the shared helper, while `conditioning-2.log` records successful conditioning. Keep both receipts and the interpreter/version in the manifest.

## Confirmed method properties

- Before and after use separate immutable binary paths in the owned repository; the baseline build manifest contains its actual captured source and 107 product input hashes. Candidate pin creation remains a future gate.
- The initial state requires 100k knowledge, 100k source nodes, 500k edges, 100k physical vectors and zero embedding failures. Subsequent calls compare all captured counts, revision and vector identity. Proxy environment variables are removed and loopback is excluded from proxy routing.
- Every CLI run is a fresh process. The wall clock includes process spawn through process/pipe completion and cleanup. CPU/RSS are per-child wait4 data, correctly excluding the independently running Ollama daemon; RSS is lifetime maximum, not simultaneous process-tree peak.
- The conditioning call (144.235049 seconds) is retained and excluded. Screening for each L and omitted/above-100 smoke calls are also retained but excluded from medians.
- Three fixed pairs per limit use before/after, after/before, before/after. Each sample receipt is written before result assertions; there is no retry or outlier removal loop. `pairs` creation fails if the directory exists, preventing silent overwrite. Preserve failed campaigns and use a fresh directory if an actual driver fault requires a new campaign.
- Summary arithmetic uses median(after), median(before), speedup=before/after and latency reduction=(before-after)/before*100. This is a ratio of side medians, not a median paired speedup; label it accordingly. n=3 supports a local descriptive comparison, not confidence intervals or tail-latency claims.
- The bounded oracle returns 1 FTS hit plus 5 or 100 vector hits, yielding 6 or 101 unique candidates. It uses SQL joins/order/limits and exact rational RRF independently of the Rust ranking implementation. The baseline oracle computes full-channel ranking inside SQL. Both are valid for a clean unfiltered corpus; neither independently tests restrictive-filter semantics.
- Retained oracle IDs/order happen to be identical before/after at both limits. The driver nonetheless correctly validates each side against its own oracle. This result is specific to this query; bounded semantics are not generally identical to full-corpus semantics.
- SQL candidate counts certify the independent SQL oracle's row bound. They do not instrument how many candidates the product observed; product-side observation bounds require the reviewed code and focused tests. Likewise no bounded SQLite internal memory or scan count is implied.
- Repeated state reads and explicit conditioning warm parts of the OS/database cache. The campaign should be reported as a warm, locally conditioned workload with uncontrolled shared machine/model cache state. Only one narrow query is measured; it has a single lexical hit, so this is chiefly the vector/full-candidate-work reduction scenario.

## Remaining completion gates

Await candidate pin/build, actual-root verification and host runner, the external reservation/fixture freshness evidence, ordinary sample receipts and final aggregate arithmetic. Genuine Samply must remain separate from ordinary timing. No speedup or performance acceptance is certified by this method-only review.
