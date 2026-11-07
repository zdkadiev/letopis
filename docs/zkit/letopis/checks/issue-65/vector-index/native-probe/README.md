# Native vector-index diagnostic archive

[Report](report.md) contains the finalized experiment. This archive preserves original evidence bytes from `.artifacts/issue-65/vector-index-2026-11-07/native-probe/`; it excludes databases, WAL/SHM, dylibs and builds. The original artifacts remain untouched.

The query is **a recorded stored embedding from rowid1**,768 f32 dimensions, saved in `query.f32`; it is **not the actual provider-produced query used in whole-CLI measurements**. These native vec1 SQL timings use diagnostic SQLite CLI **3.54.1**. The product embeds SQLite **3.53.2**, and its complete CLI behavior/timing must be assessed separately. No external documents were consulted for this experiment; pinned local source and oracle hashes are in [copy-receipt.json](copy-receipt.json).

Raw query outputs and cleaned row outputs are gzip-compressed with mtime0. [archive.json](archive.json) records each original filename, archive filename, uncompressed byte count and SHA-256, compressed byte count and SHA-256, exclusions and receipt checks. The failed v1 warmup receipt/output remain preserved as failure evidence; it contributes no accepted sample. `runs.json` contains only the24 matched native measurements. Corrected corruption controls are in `control-v2-runs.json`; earlier formatting-only controls are preserved for provenance.

Run this metadata-only verifier from any working directory:

```sh
python3 -B /absolute/path/to/this/archive/archive-verify.py
```

It checks compressed and original hashes, gzip mtime0, receipt output hashes/cardinalities, all24 result prefixes and full-vector hash equality without accessing databases or running SQL. The unchanged original `validate.py` and probe scripts are retained for provenance; they expect uncompressed result filenames and the original experiment DBs. To use the original validation script independently, decompress result files into a separate temporary working copy first. No product build, test, benchmark or provider call is part of archive verification.
