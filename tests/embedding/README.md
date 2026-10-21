# TK-3: vec1 snapshots, text identity and durable attempts

Retained scratch Rust integration checks for MC-3/MC-6, FR-29/FR-30/NFR-7. This is a bounded test implementation, not the production executor/config/parser. All corpora, caches and vector databases are temporary.

## Reproduce

From the repository root on macOS or Linux with Python 3, rustup and a C compiler:

```sh
python3 -B tests/embedding/run.py
```

Use `--evidence NEW_DIRECTORY` to retain a named receipt; existing destinations are refused. The runner pins Rust 1.95.0, rusqlite 0.40.2, libsqlite3-sys 0.38.2 / SQLite 3.53.2 and native vec1 0.8 from SQLite commit a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7. It uses TK-1's source checksum verifier and this crate's resolved SQLite headers, fresh native library/database files, and separate OS/architecture Cargo targets. It records every consumed package input hash, exact commands/environment/exit codes and full logs. It runs 11 Rust integration tests, two Python process-lifecycle regressions, fmt and Clippy, then the complete TK-2 runner (including TK-1) and roadmap checker.

Use the unchanged TK-2 Docker image for actual Linux ARM64 execution:

```sh
docker build --platform linux/arm64 -t letopis-tk2:rust-1.95.0 tests/publication
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD,target=/repo" \
  --workdir /repo/tests/embedding letopis-tk2:rust-1.95.0 \
  python3 -B run.py --evidence /repo/docs/zkit/letopis/checks/TK-3/linux-arm64
```

If a successful host build has populated the Cargo source registry, an offline Linux build can additionally mount `$HOME/.cargo/registry` read-only at `/task-cargo/registry` and set `CARGO_HOME=/task-cargo`, `CARGO_NET_OFFLINE=true`, `RUSTUP_TOOLCHAIN=1.95.0-aarch64-unknown-linux-gnu`. These are Linux builds from source; no host binaries are reused. The pinned vec1 source must be cached or its initial download needs network access.

## Fixture interfaces

The first Markdown line is `<!-- {"links":[]} -->`, followed by search text. One file is one node; TK-2 handles fixture publication. Config fixtures have provider `fixture`, model `three` (3 floats) or `four` (4 floats), empty parameters, and integer max_retries. Identity uses the same sorted string/integer JSON subset as TK-2, not general JCS. No production dimension field is added.

`embedder ROOT MODE CHECKPOINTS` is a controlled child, not a supported product command. The provider runs in that child and appends/fsyncs `.letopis/requests.jsonl` solely as an independent test observation. Valid output is the first unit vector, except text `bravo` uses the second unit vector. Modes `error`, `empty`, `nan`, and `wrong` return a deterministic error or invalid output. No actual model, HTTP request, credential or detached launch is involved.

Runner commands start in a separate POSIX session; a timeout or interruption kills the whole command process group and reaps the direct child. Two real subprocess regressions use socket EOF to prove a descendant terminates on timeout and SIGINT. They clean up descendants even when a regression is detected. Deliberately daemonized processes that create independent sessions are not part of this command-group guarantee.

The child flushes `checkpoint:NAME` and waits for a parent stdin line at selected comma-separated checkpoints: selected, reserved, before_request, after_response, store_key, store_vector, before_store_commit, after_store_commit. The parent controls edits/resets and actual SIGKILL, verifies signal exit, uses 20-second control/exit deadlines and kills/reaps abandoned children. No sleep schedules a race. A 5ms exit poll only bounds process cleanup.

## Observable contract

- Cache and vectors are real separate WAL databases; vectors.db is accessed through ATTACH AS vec for selection/retrieval. Each connection enforces FULL and foreign keys; macOS also uses fullfsync. Writes for reserve/store/failure/reset use a dedicated vectors.db connection so BEGIN IMMEDIATE does not lock the main cache.
- A pinned old reader keeps old native vector bytes, config identity and a prepared nearest-neighbor query across a committed dimension reset. A fresh reader sees the new dimension. An aborted reset retains keys/vectors/failures. If only the cache snapshot was pinned before reset, the later vec identity check yields FTS-only. Two database files are not claimed to have one atomic snapshot.
- The eligible-work SQL selects knowledge hashes without a vector and without attempts >= 1+max_retries. Hashes are deduplicated; source nodes are excluded. A live-file re-read that differs from the selected hash skips that hash for this run without consuming an attempt, preventing an infinite loop. Work is queried again after each step.
- A late vector is saved under its captured text hash. Retrieval queries real native vec1 and joins vector rowid -> vector_keys -> current nodes.search_hash. An edited node cannot inherit an old hash's vector. Identical texts share one vector, including after rebuilding the main cache.
- Reservation increments attempts in a committed vectors.db transaction before invoking the provider. Storage atomically publishes a key and vector and removes the failure row. Config identity is rechecked inside every reserve/store/failure transaction; stale results/errors cannot modify the new config, including a same-dimension reset.
- Empty/nonfinite/wrong-dimension output and provider errors consume the same finite attempt budget. A crash after reservation can consume an attempt without a request; a crash after a lost response can cause another request if budget remains. Exactly-once provider invocation is not promised.
- When no eligible work remains, unreferenced vectors/failures are collected. This fixture reads referenced hashes first and then performs identity-guarded vector-only cleanup; it does not prove reclamation against concurrent corpus publication. A referenced shared old hash is retained.

## Checks and limits

Five snapshot tests and six drain tests include 30 SIGKILL cuts: 9 for max_retries=0 and 21 for max_retries=2, over the eight checkpoints above. Independent assertions inspect literal fixture hashes, exact native bytes/key membership, attempts/error rows, provider request logs and current node joins. Same- and different-dimension config races cover reservation, response and late-error schedules.

The fake provider retries immediately to keep deterministic checks fast. Real HTTP backoff/transport belongs to TK-6; production executor/config to TK-13/TK-27; launch/flock to TK-4; bulk/drills to TK-37. Full Markdown/anchors, general JCS, arbitrary concurrent in-place file writes, corrupt cache repair, cross-file atomic transactions, physical power loss, x86-64, capacity and retrieval quality are outside this proof.

See the [TK-3 card](../../docs/zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-3-vector-harness.md) and [execution report](../../docs/zkit/letopis/checks/TK-3/report.md).
