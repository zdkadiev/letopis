# vec1 flat write-path check — source only

Pinned source: `vendor/vec1/vec1.c` (vec1 0.8), SHA-256 `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`. This note inspects C source only. No SQL, build, timing, provider call, new benchmark or full reembedding was run. It does not change the archived270-file experiment or its manifest.

**Conclusion:** an ordinary flat insert does not incrementally append bytes to an existing index BLOB. It can load a **small** block, append in memory, and replace that complete block at transaction sync. With the pinned defaults, this is not a rewrite of an existing approximately1 MiB rebuilt block for every singleton insert: the reuse threshold is a separate **16 KiB `blocksize-min`**, while `blocksize` is1 MiB. Physical SQLite page/WAL writes and drain throughput remain unmeasured.

## Exact insertion and commit path

1. `vec1UpdateMethod`, lines11813–11820, creates a transaction writer only for indexed modes. Lines11837–11845 write the base row: flat stores the bucket integer; none stores the full incoming vector BLOB. Lines11858–11864 additionally call `vec1WriterVector` for flat. None has no index writer/packed-block maintenance on this path.
2. `vec1WriterVector`, lines10478–10503, initializes its builder and attempts to reuse an existing block with `length(val) < cfg.nBlocksizeMin`. The actual `VEC1_SQL_SEARCH_IDX_FOR_INSERT` query is at lines5930–5932: same bucket, length filter, `limit 1`. It is not a request to append to the most recent approximately1 MiB block. `vec1ListBuilderLoad`, lines9930–9979, copies the existing rowid/vector payload into memory before appending.
3. `vec1ListBuilderAdd`, lines10260–10330, appends the new vector/rowid to memory. It flushes first if the prospective payload would exceed `blocksize` or the rowid representation must change. Defaults at lines1880–1881 are1 MiB block size and16 KiB minimum; `vec1LoadConfig`, lines8547–8578, installs defaults and reads optional overrides.
4. `vec1SyncMethod` → `vec1FinishWriter`, lines11877–11892, finishes the transaction writer; `vec1WriterFinish`, lines10864–10876, flushes each builder and frees it. A following transaction therefore starts with a fresh writer rather than retaining the previous builder.
5. `vec1ListBuilderFlush`, lines9997–10046, allocates and serializes the **whole current block** (header, every rowid, every vector). If replacing a loaded block, it executes `delete from ..._idx where id=?`, then inserts the complete new BLOB through `sqlite3_bind_blob`. It does not call `sqlite3_blob_write` to append the new vector.

For a fresh index, monotonically increasing ordinary4-byte rowids, no metadata columns and768-dimensional f32 vectors, one vector contributes3072+4 bytes and the block header is12 bytes (`VEC1_LIST_SZHDR`, line5456). Source arithmetic gives5 vectors=15,392 bytes (<16 KiB); after the sixth,18,468 bytes (≥16 KiB). With one insert per committed transaction and no other reusable undersized blocks, subsequent inserts start another small block. This is a conditional layout inference from the code, **not a measured write-amplification result**. A bulk rebuild can retain one builder while filling near1 MiB, so a rebuilt index and an index grown through singleton commits can have different block counts/layouts. The earlier native query experiment measured the rebuilt layout.

## Updates and incremental writes

`vec1UpdateMethod`, lines11799–11806, treats updating an existing row as deleting the old entry and inserting the new one (deletion ordering depends on whether the rowid changes). In `vec1DeleteByRowid`, lines11703–11717, the normal delete path can use incremental `sqlite3_blob_write` to replace the rowid with a4-/8-byte tombstone and update a4-byte tombstone count. Above the tombstone threshold it instead loads, compacts and flushes the list, lines11670–11699. These incremental writes are **delete maintenance**, not the insertion append path.

## Reporting limit

The source establishes whole-small-block replacement at vec1's SQL boundary; it does not establish exactly how many SQLite pages, WAL bytes, disk bytes or fsyncs that replacement causes. Added block lookup/base/index maintenance may affect writes, but neither its net cost nor full drain throughput was measured. The approximately142-minute100k drain baseline belongs to the earlier mode/provider experiment. Full100k reembedding after switching to flat was not rerun, so no equal, better or worse throughput claim follows from the fast read/rebuild measurements.
