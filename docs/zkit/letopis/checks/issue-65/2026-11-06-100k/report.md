# 100k corpus and real Ollama attempt

Requested 2026-11-06. Continues the [previous diagnosis](../current/report.md).
This is a new experiment; earlier timeout receipts and profile results remain unchanged.

## 100k indexing and command results

The optimized release successfully initialized **100,000 knowledge nodes,
100,000 distinct source addresses and 500,000 edges** in **162.648 seconds**.
The initial-publication limit was explicitly raised from 120 to 600 seconds.
This resolves the earlier question of whether initialization can finish within a
larger window; it is not an initialization speedup claim. Input verification found
73,333 metavertex and 26,667 vertex rows, plus 100,000 source rows.

That first invocation consumed 154.485 CPU seconds,
with 431.45 MiB wait4 lifetime peak RSS.
The first full inspect capture took **52.522 seconds**,
with 60.55 MiB peak RSS. Those n=1
observations are separate from repeated initialized command timings below.

All numeric command timings in this section have **embeddings disabled**.
Each invocation starts a fresh CLI process in an already initialized synthetic
repository. OS page cache is uncontrolled. Repeated inspect uses already captured
objects; it must not be labeled first capture or cold start.

| Command | n | Median, seconds | Min–max, seconds | Max CLI RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| search | 3 | 0.7024 | 0.6309–0.7376 | 17.84 |
| traverse | 3 | 0.7024 | 0.6878–0.7083 | 18.42 |
| inspect | 3 | 8.2599 | 8.2135–9.3595 | 60.58 |
| check | 3 | 40.9503 | 40.3766–41.8228 | 376.98 |
| status | 3 | 40.7023 | 40.4185–40.7184 | 374.73 |
| index --full | 1 | 207.4024 | 207.4024–207.4024 | 424.20 |

Raw data: [initial calls](initial-samples.jsonl), [initial manifest](initial-manifest.json),
[direct calls](direct-samples.jsonl), [direct manifest](direct-manifest.json),
[summary](direct-summary.json). Initialization was one successful attempt, not a
stable distribution. n=3 describes a small local sample; no p95/p99 is estimated.

Correctness checks verify exact graph cardinalities for every direct call, the
literal FTS target, traversal IDs/20 expected edges/clean flags, the independently
frozen inspect range/hash and bytes from its retained Git tree, empty check errors,
clean status locks/problems and a positive publication revision for full indexing.
No source or knowledge content was edited for these measurements.

The general survey initially made fresh repository copies between scenarios;
copying changes file identities and its refresh adds substantial setup cost at
100k. It was deliberately interrupted after retaining successful initialization,
search and first-inspect receipts ([completion](initial-completion.json)). All
owned fixture index locks were observed released. The direct campaign then used
the verified, already initialized/captured inspect repository for read commands,
and finished with one full-index attempt. Setup/copy time is excluded from CLI
latency and is not attributed to query execution.

## Paired inspect comparison at 100k

Three alternating before/after pairs on the same already initialized/captured
quiescent corpus measured **40.957 s → 7.963 s**, a **5.14× speedup / 80.56% lower
median latency**. Before min–max: 40.100–42.077 s; after: 7.922–9.423 s.
All six results passed the independent location/hash/snapshot/graph-count oracle.
The projection revision and returned complete snapshot tree were unchanged.

The binaries have identical storage/schema/provider inputs and differ only in the
three Git implementation files; their hashes and exact source inputs are pinned.
This comparison uses the same repository sequentially because read/capture state
is logically unchanged and durable objects/refs are idempotent. It has no additional
conditioning calls, follows prior successful captures, and is not a cold-start
comparison. n=3 is a small local sample; no tail quantiles or formal SLO verdict.

[Samples](inspect-pairs-samples.jsonl), [summary](inspect-pairs-summary.json),
[manifest](inspect-pairs-manifest.json), [driver](compare-inspect.py).

## Remaining full-check cost at 100k

A separate valid instrumented full check took **40.534 seconds**. The temporary
Rust wall-time timers recorded 391 source-observation batches occupying **38.735
inclusive seconds**. Within those batches, **100,000 historical snapshot reads**
occupied **29.974 s**, and **100,000 confined live reads** occupied **8.671 s**.
Git addressed reads account for 29.768 inclusive seconds within the historical
scope; filesystem captures account for 8.551 seconds within the live scope.
These are nested inclusive times: do not sum parents and children or call them
CPU percentages. The read-only inspection workload and complete graph cardinality
checks remained valid. The dump locates a major remaining full-check cost; it does
not establish that caching live observations would preserve freshness.

[Raw dump/build identity](check-profile.json), [driver](profile-check.py),
[exact instrumentation patch](../current/timing-instrumentation-after.patch).
This diagnostic execution follows the ordinary benchmark and is not included in
its latency series or a paired overhead estimate. It is an instrumented Rust
wall-time profile, not successful native sampling or exact SQLite wait accounting.

## Ollama: attempted, blocked by sandbox networking

The user explicitly authorized a real Ollama run. The general FTS runner retains
its canned model-free authorization labels; the actual authorization and networking
failure for this request are recorded separately. The locally installed model
`embeddinggemma:300m-qat-q4_0` was selected. Its model manifest and local referenced
weight files exist; no model was downloaded. The loopback API at
`http://127.0.0.1:11434/api` cannot be contacted from this executor:
**PermissionError / errno 1 / Operation not permitted**.
This does not establish whether the user's Ollama daemon is running outside the
sandbox. [Connection receipt](ollama-connection.json).

A bounded 60-node product probe confirmed the failure through the real provider
path, using a private synthetic repository, nested enabled configuration and
`max_retries:0`. Proxy environment routing was disabled for the loopback probe.
An embedding lock lease prevented detached background drains during setup and
query calls; the document drain ran directly as an owned process.

- [Index](ollama-index.json) succeeded with the expected two Markdown files.
- [Search](ollama-query.json) returned the literal FTS hit. Successful search with
  zero vectors does **not** demonstrate hybrid/vector participation.
- [Direct drain](ollama-drain.json) exited successfully as a command, but
  [status](ollama-status.json) reported **60 exhausted embedding failures**.
- Independent read-only ordinary SQLite metadata showed **0 vectors and 60 failed
  search hashes** ([coverage](ollama-coverage.json)). No native vector dimension,
  semantic relevance, model throughput or model RAM result is claimed.

The 100k real embedding population and true hybrid measurements are therefore
**not assessed**. We stopped at the small failing probe rather than exhausting
100,000 attempts against a permission-blocked endpoint. The Ollama portion requires
execution outside this sandbox with loopback API access. Embedding-enabled FTS
fallback is not reported as successful hybrid performance.

## Source identity, scope and reproducibility

The immutable optimized binary is the release for local measurement commit
`3b2e477cd2bb8bd204ab606a1566f32e713b4ac8`, SHA-256
`77f8a66d0b0edf76799cb562537839d78fca20a2e78784c7dfe434ec9a8c7e8f`.
All 106 product input hashes match the changed checkout. No new product source,
public API, dependencies or model execution policy changed in this follow-up.
User edits and previous evidence were preserved.

Corpus: seed 65, exactly 2,000 UTF-8 bytes per knowledge fragment, 3,334 Markdown
files, 30 elements/file except the final 10, and 1,000 sharded source files.
Every canonical input hash was checked before the direct campaign. Host/tool
identity is retained in the manifests; the host is not isolated.

RSS is the wait4 lifetime high-water mark for the direct CLI child and
OS-accounted reaped descendants, not simultaneous process-tree or whole-stack/model
RAM. CPU is wait4 accounting; block counters are not I/O bytes. Real model CPU,
GPU, memory and latency remain unavailable. Changed-file/mass-edit scenarios,
real-provider contention, Linux/x86_64 and production workloads were not measured.

The [direct driver](direct-run.py) and [Ollama probe](ollama-probe.py) are archived
experiment code, with exact receipts and hashes in [evidence index](evidence-index.json).
They refer to preserved local input paths and require new output paths for a new
attempt. Historical [runner documentation](../../../../../../tests/performance/README.md)
explains the underlying measurement/oracle helpers. The actual command was the
optimized clone's `run.py --stage scale --nodes 100000 --repetitions 1
--initial-timeout 600 --timeout 120`, followed by the direct driver.
