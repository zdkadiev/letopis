# SQLite capacity probe, 2026-10-18

Isolated resource measurement authorized by D-57. See [R-6](../R-6-sqlite-capacity-probe.md) for results and limitations. This is measurement code, not an Engram implementation or an approved physical schema.

## Evidence

- `schema.sql`, `src/main.rs`: exact loaded fixture, checks and single-reader measurements.
- `src/bin/mixed.rs`: ten simultaneous reads, then 150 scheduled storage requests at 10/s.
- `src/bin/parallelism.rs`: identical offered load with 1, 2 and 4 reader connections. Static round-robin dispatch; not a product queue or a claim of optimal scheduling.
- `*-stdout.jsonl`: every captured query, phase, completion and verification result.
- `*-time.txt`: macOS `/usr/bin/time -l -p`; maximum RSS and peak physical footprint. No OS cache purge was performed.
- `*-memory.json`: sampled process RSS; short peaks may be missed. Prefer time-reported maxima where available.
- `*-summary.json`: exit codes and timeout checks.
- `analysis.json`: recomputed nearest-rank p95, median, maximum; scheduled-to-finish includes dispatch lateness. `analyze.py` recomputes it without opening the database.
- `db-breakdown.json`: read-only dbstat page accounting, free pages, metadata lengths and additional structural checks. `inspect_db.py` reproduces it; its observer SQLite 3.53.4 differs from the measured Rust runtime 3.53.2 and did not modify the database.
- `preflight.json`, `build-manifest.json`, `first-build.json`, `Cargo.lock`: build, hardware and source provenance. Initial main binary remained unchanged during later follow-ups. The final manifest also records the other two binaries.
- `sha256.json`: SHA-256 of all preserved files except this manifest itself.

Not preserved: generated 687,943,680-byte database, downloaded crate cache, targets, native binary/library, and external vec1.c. Their exact source versions/hashes are recorded; generated content is reproducible from the fixture. The temporary original was `/private/tmp/engram-sqlite-capacity-idny94a4`.

The main fixture creates 200k nodes, 500k edges, 100k FTS entries and 100k vectors. Knowledge text is 2000 ASCII bytes; vectors are synthetic normalized f32, not model embeddings. Graph/source hash values and tree OIDs are sizing placeholders. Source nodes have no FTS/vector entry. Two candidate lookup indexes on edge endpoints and flat/cos vec1 are probe conditions. Full domain validation, filtering, scoring/fusion, Markdown parsing, files/Git writes, jobs, errors, configuration and recovery tables are excluded. No concurrent writes or Ollama work occurred.

## Reproduction on macOS arm64

Use a **new empty temporary directory**, not the canonical documentation folder or a project DB. The archived runners contain the executed absolute ROOT path; in a disposable copy, set ROOT to the new directory. They overwrite result files. Never rerun them against the preserved evidence.

1. Copy Cargo.toml, Cargo.lock, schema.sql, src/, run_probe.py, run_mixed.py, run_parallelism.py, analyze.py and inspect_db.py into the new directory. Install Rust 1.95.0 and macOS command-line tools, record OS/hardware and SQLite header version/path. The measured header directory was `/opt/homebrew/opt/sqlite/include`.
2. Download the exact source below as vec1.c and verify SHA-256 **before compiling**:

   Source: <https://raw.githubusercontent.com/sqlite/sqlite/a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7/ext/vec1/vec1.c>

   SHA-256: `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`.

3. Inside the disposable directory:

```sh
/usr/bin/clang -O3 -DNDEBUG -dynamiclib -fPIC -I/opt/homebrew/opt/sqlite/include vec1.c -o vec1.dylib
CARGO_HOME="$PWD/cargo-home" CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked --bins
```

4. Set the three copied runners' ROOT to that directory. Run them sequentially with `/opt/homebrew/bin/python3 -B`: `run_probe.py`, `run_mixed.py`, `run_parallelism.py`. Their individual deadlines are 300/120/120 seconds; these are not a combined five-minute guard. The recorded executions total 101.725 seconds of program time (103.04 seconds of time-wrapper real), below the approved combined five-minute load budget. A reproduction needs its own combined deadline and authorization if taking longer. Dependency download/build is separate.
5. Run `inspect_db.py` against the newly generated DB and save stdout as db-breakdown.json. Save that run's own build manifest; do not reuse historical binary hashes. Run `analyze.py` after all phases. It asserts counts, query counts/indices, time accounting and the 300-second summed program bound; it does not assert a one-second latency success. Ten-reader and one-reader cases exceeded that target.

The artifact manifest can be checked with Python's hashlib. Do not treat byte-identical binaries, allocated pages, timings or memory as portable guarantees across toolchains, machines and library versions. The measured database was subsequently inspected read-only, not vacuumed. OS page cache remained warm; no four-GB device was tested.
