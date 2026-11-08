# Pinned vec1 native none/cos versus flat/cos — 2026-11-07

**Result:** `flat/cos` is a promising exact physical-mode change. In this bounded native SQL experiment, full `K=100000` median query CPU fell from **2.145223 s to 0.107106 s (20.03×)**. All24 outputs (3 paired repetitions ×4 K values ×2 modes) match the same full-result prefix byte-for-byte in row IDs, order and17-digit formatted distances. The ordered SHA3-256 of **all100,000 complete768-dimensional vector BLOBs** also matches after rebuilding. This is not a whole-product CLI speedup claim.

No product code/build/provider/reembedding was used or changed. No shared fixture/evidence was mutated. Native work is complete; only this owned directory was written. The directory is approximately884 MB, below the3 GB budget. Native timed queries and rebuild took seconds; heavy SQL work was bounded and has stopped. Both DB copies are available for the coordinator's independent whole-CLI experiment:

- `none.db`: SQLite online backup of the shared populated fixture.
- `flat.db`: online backup of `none.db`, then one atomic vec1 rebuild.

## Source semantics before measurement

Pinned implementation: `vendor/vec1/vec1.c`, version0.8. Hashes for the source and the existing loadable oracle are recorded in [copy-receipt.json](copy-receipt.json). Relevant source locations:

- `vec1FlatIndexCfg`, lines3238–3276: rebuild accepts `none` or `flat`, with cosine available for both.
- `vec1QueryToSql`, lines9079–9160: `none` delegates to `select *, vec1_cos_distance(...) ... order by d limit K` over raw base rows.
- `vec1SpecialInsert`, lines11446–11545: `flat` sets `VEC1_MODEL_INDEX` with zero codebooks and zero coarse buckets, then rebuilds the index.
- `vec1DoKANNBucket`, lines8315–8428: without PQ/BQ, scan every full f32 vector in the packed index and call `vec1CosDist` for cosine, exactly as the scalar cosine function does.
- `vec1DoKANNQuery`, lines8435–8525: select candidates with an in-memory heap and sort the result. The documented heap allocation is `K*16` bytes, approximately1.6 MB at100k.
- `vec1RebuildIndex`, lines11140 onward, and `vec1GetVector`, lines9420 onward: flat relocates full raw vectors into index blocks; the base retains bucket metadata, while virtual-table vector reads reconstruct the original BLOB.

Thus **flat here is exact exhaustive search with a different layout/execution path, not ANN**. Both modes still evaluate every vector. The none implementation's internal `select *` carries complete BLOBs through its ordered/limited SQL result; flat uses packed block scanning and a compact result heap. This is a source-based explanation for the particularly large full-K difference, not a measured attribution of each subcost.

## Copy and query provenance

Homebrew CLI `/opt/homebrew/opt/sqlite/bin/sqlite3` version3.54.1; load existing `.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib` explicitly with entry `sqlite3_extension_init`. This CLI's SQLite version is not the product's bundled SQLite version; the vec1 source is pinned and the comparison uses one identical CLI/extension in both arms.

Original source: `.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository/.letopis/vectors.db`. Source opened with `-readonly`; `.backup` uses SQLite's online backup API and includes committed WAL frames. The source WAL was observed at0 bytes before backup. Backup elapsed time was not recorded separately and is not claimed. Each owned DB is approximately407 MiB; own WALs are0 bytes after connections close.

The saved query [query.f32](query.f32) is the3072-byte virtual-table vector from rowid1 of the owned baseline copy, selected by minimum `vector_keys.row_id`. It is a **real stored embedding used as a query**, not the provider-produced query from the previous CLI experiment. No model/provider was called.

Native query shape for each K:

```sql
select rowid, printf('%!.17g',distance)
from vector_index(readfile('/absolute/owned/query.f32'),'{K:100000}')
order by distance,rowid;
```

The complete query scripts, raw outputs including `.timer` lines, cleaned row outputs, stdout/stderr and per-run receipts are retained. Neither the graph join nor application filters/provenance/fusion are part of these timings. Tie ordering here is by vector rowid; product ranking ties by knowledge ID. Full identical per-vector distances permit that product ordering, but the product path still needs its own validation.

## Matched pairs and equality

One warmup per mode preceded measurements. Each K has three matched pairs, alternating arm order `none→flat`, `flat→none`, `none→flat`. Each arm opens a fresh read-only CLI process, with warm OS cache. SQLite `.timer` measures only the SQL request and result serialization, excluding extension loading; CPU is `user+sys`. Process wall/user/system measurements are also retained separately.

| K | none CPU median, s | flat CPU median, s | CPU speedup | none wall median, s | flat wall median, s |
| --- | ---: | ---: | ---: | ---: | ---: |
|5|0.087641|0.054451|1.61×|0.087841|0.054520|
|100|0.088012|0.057254|1.54×|0.088188|0.057421|
|1000|0.133266|0.056556|2.36×|0.134313|0.056858|
|100000|2.145223|0.107106|20.03×|2.402859|0.135671|

Raw measurements: [runs.json](runs.json); all samples, per-pair output hashes and comparisons: [summary.json](summary.json). [validate.py](validate.py) rechecks saved evidence without running SQL or mutating DBs.

All24 result files equal the appropriate prefix of the baseline full100k output. No membership, rank-order or formatted-distance discrepancy was found. Distances are serialized with17 significant digits. These normal-fixture comparisons do not prove error semantics for arbitrary corrupt storage; separate raw-typed controls below avoid treating a NULL distance as numeric zero.

Full vector-byte verification independently queried virtual-table `(rowid,vector)` in rowid order and hashed SQLite's deterministic `sha3_query` serialization. Both returned100,000 rows and307,200,000 vector bytes:

```text
4ED319020B3ADFE2FD6FC375EE308B6266F2D9E74689A6DE1EF7E22488218EE4
```

Receipts: [vector-hash-none.receipt.json](vector-hash-none.receipt.json), [vector-hash-flat.receipt.json](vector-hash-flat.receipt.json). This checks full BLOB bytes, not merely matching search results. Hash scans took2.066/4.027 process-wall seconds and were separate from timings.

## Rebuild, layout and corruption controls

Owned flat rebuild:

```sql
insert into vector_index(cmd,arg)
values('rebuild','{index:"flat",distance:"cos"}');
```

[Rebuild receipt](rebuild-flat.receipt.json): SQLite wall2.174718 s, CPU1.230616 s (user0.313094 +sys0.917522). Physical file size remained approximately407 MiB, reusing free space. Metadata shows295 packed index blocks holding307,603,540 bytes; virtual-table vector bytes remain307,200,000. Base shadow `vector` values now hold bucket metadata, so direct consumers of that shadow column cannot assume it remains a BLOB. Application consumers must use the virtual table.

Both copies returned `pragma quick_check=ok`; exact native index-specific integrity/migration/API tests were not added to the product in this experiment.

Cheap controls used three **separate tiny owned DBs**, with three2-dimensional vectors each and direct shadow-table corruption at row2. [controls-v2.py](controls-v2.py) and [control-v2-runs.json](control-v2-runs.json) preserve raw distances and their SQL types:

| Injected stored corruption | none full query | none→flat rebuild | after rebuild / raw reads |
| --- | --- | --- | --- |
|3-byte vector instead of8 bytes|SQL error|Rejects `unexpected vector size in vector_index_base: 3`; statement rolls back|Model remains none; query still errors; raw malformed bytes unchanged|
|Quiet NaN f32 plus1.0|Returns SQL NULL distance for corrupt row|Succeeds|Flat also returns SQL NULL; original bytes unchanged and exposed through virtual table|
|Positive infinity f32 plus1.0|Returns SQL NULL distance for corrupt row|Succeeds|Flat also returns SQL NULL; original bytes unchanged and exposed through virtual table|

**Important:** rebuild validates vector length but does not itself reject these injected nonfinite stored components. Neither native mode repairs them or raises an error merely when reading raw vectors. For full K both modes expose NULL distance; the product's typed `rusqlite` f64 decode is expected to reject NULL, but that product behavior was not executed here. Preserve existing product finite/type/error checks and test them before migration is accepted.

The first control formatting used only `printf`, which formats NULL as `0.0`; raw evidence is preserved. The corrected v2 control explicitly returns `distance` and `typeof(distance)`, proving NULL is not a genuine zero distance. The first warmup also revealed SQLite shell3.54 redirects `.timer` output into `.output`; its6-line file for5 rows and failed receipt are preserved. The corrected driver separates timer lines from data, and every accepted run has exactly K data rows and one timer record. Neither correction changed DB vectors or timed-query behavior.

## Trained mode: research only

No training was run. Pinned source supports `vec1_train(VECTOR,CONFIG)` returning a model BLOB accepted by the same rebuild command. Training supports `quantizer:none/pq/opq/bq`, `nbucket`, code size and other parameters; `trained` is not a magic string accepted by the simple `none/flat` rebuild configuration parser.

- Defaults: no coarse buckets and no quantization; cost/accuracy depends on the explicit configuration. Training collectors hold full input vectors, approximately307.2 MB for this100k×768 fixture before extra buffers/transforms. Source comments permit chunk accumulation up to1024×32 MiB; the nearby prose says64 GiB, but the constants multiply to32 GiB, so use code rather than that comment.
- Coarse IVF training requires at least `4*nbucket` samples. PQ/OPQ additionally require at least512 samples. K-means uses a30-iteration ceiling; OPQ adds rotations and covariance/SVD work. No actual training latency, peak memory or useful sample size was measured.
- With buckets, default `nprobe=0.05` scans roughly5% of buckets, at least one, which can omit otherwise eligible candidates. **`nprobe:1` means one bucket, not100%;** the parser interprets values≥1 as an integer bucket count. Exhaustive bucket search requires the explicit bucket count.
- With PQ/OPQ/BQ, distances are computed from compressed codes and can change ranking even when every bucket is searched. Current exact cosine/full-eligible-channel semantics are not automatically preserved by training.
- `quantizer:none` plus scanning every bucket uses full-vector cosine rather than PQ/BQ code distances, but numerical equality still needs verification and bucket pruning supplies no selective-search acceleration. Any future selective or quantized path needs an agreed recall/rank contract, representative queries, independent exact baselines and current-hash/config/filter handling before product adoption.

## Scope and handoff

The native result justifies the coordinator's next experiment with the immutable current product binary against separately copied fixtures. It does not authorize truncating vector channels to K5/100/1000 in product search; those values only diagnose native behavior. Product search still requires complete eligible channels before rank fusion/limit.

The experiment made no product changes, so no product build, `make verify`, storage runner, migration acceptance or release claim is made. Root/shared target directories, populated fixture, old evidence and collaborators' edits remain untouched. Heavy CPU activity has stopped; report/receipt writing is the only remaining work.
