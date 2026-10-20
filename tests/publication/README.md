# TK-2: publication and process interruption checks

Retained Rust integration harness for MC-2/D-95 publication, full reindex, and the transaction boundary of MC-3 config reset. All corpora and databases are temporary. This is a test implementation, not the production CLI/storage/parser.

## Reproduce

Requires Python 3, rustup and a C compiler on macOS or Linux. From the repository root:

```sh
python3 -B tests/publication/run.py
```

The runner pins Rust `1.95.0`, rusqlite `0.40.2`, libsqlite3-sys `0.38.2` / SQLite `3.53.2`, and vec1 `0.8` from SQLite commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`. It imports TK-1's checksum verifier and vec1 source pin, compiles against this crate's resolved SQLite headers, runs all TK-2 tests, fmt and Clippy, then runs the **complete TK-1 runner** and the roadmap checker. Other dependencies are pinned in Cargo.lock. No extension or test database is reused between runs.

An explicit evidence destination must not exist:

```sh
python3 -B tests/publication/run.py --evidence /tmp/tk2-macos
```

For actual Linux ARM64 processes in Docker, build the test image and mount the whole repository because this crate consumes TK-1's helper:

```sh
docker build --platform linux/arm64 -t letopis-tk2:rust-1.95.0 tests/publication
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD,target=/repo" \
  --workdir /repo/tests/publication \
  letopis-tk2:rust-1.95.0 \
  python3 -B run.py --evidence /repo/docs/zkit/letopis/checks/TK-2/linux-arm64
```

The [Dockerfile](Dockerfile) uses TK-1's identical pinned Rust base digest and Debian build packages, and installs the pinned toolchain's rustfmt/Clippy during image build. Cargo targets are separated by OS/architecture. Evidence includes tool/platform versions, native source/header/library hashes, exact argv/environment/exit codes, the schema and consumed helper hashes, and full logs. Rust tests require the trusted library path supplied by the runner; missing vec1 fails the run.

If the container cannot reach crates.io after a successful host run has populated Cargo's registry, use the same sources read-only with an offline native Linux build (no host binaries are mounted):

```sh
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD,target=/repo" \
  --mount "type=bind,source=$HOME/.cargo/registry,target=/task-cargo/registry,readonly" \
  --env CARGO_HOME=/task-cargo --env CARGO_NET_OFFLINE=true \
  --env RUSTUP_TOOLCHAIN=1.95.0-aarch64-unknown-linux-gnu \
  --workdir /repo/tests/publication letopis-tk2:rust-1.95.0 \
  python3 -B run.py --evidence /repo/docs/zkit/letopis/checks/TK-2/linux-arm64
```

The explicit toolchain selects the image's already installed pinned Rust; runtime versions are checked again. The runner records this environment. The checked vec1 source must also already be cached, or its initial download still requires network access.

## Observable contract

- Publication uses a persistent `.letopis/index.lock` with real kernel flock. A waiting process re-detects after acquiring it. A no-change invocation does not take this lock.
- One transaction updates nodes, declared edges, contentless-delete FTS, files (stat/hash/error), and revision. Invalid files keep their previous valid projection; fixing/deleting a file clears its error. Row IDs survive updates.
- D-95 keeps all declared edges. Query-time joins hide missing targets and reveal them when the target returns. Includes cycles are diagnosed; traversal uses a visited set and terminates.
- Reindex re-reads every fixture under index.lock and rebuilds all valid projections in one transaction. An invalid file's old projection is retained, including its contentless FTS entry; a fresh empty cache has no prior projection to preserve. This does not prove recovery from a corrupt cache.
- Reset takes an independent vectors.db BEGIN IMMEDIATE transaction, rechecks config identity, recreates native vec1 and clears keys/failures atomically with the new identity. A second reset for the same identity does not erase a newly inserted sentinel vector. It works while another process owns index.lock.

## Interruption matrix

The parent waits for a flushed checkpoint line, then sends real SIGKILL and verifies signal exit. Before-commit cuts must expose the exact old committed state; after-commit cuts must expose the complete new state despite the missing success response. A new child must converge without the parent unlocking or repairing anything.

| Operation | Checkpoints |
|---|---|
| publication (10) | detected, locked, redetected, begun, cleared, projections, fts, fingerprints, before_commit, after_commit |
| full reindex (9) | locked, redetected, begun, cleared, projections, fts, fingerprints, before_commit, after_commit |
| config reset (6) | reset_detected, reset_begun, reset_cleared, reset_identity, reset_before_commit, reset_after_commit |

Before-commit oracles compare ordered nodes, edges, every files field, revision, and every FTS token instance against a known initial state. Post-commit oracles check literal fixture projections and SHA-256 hashes, current stat/hash/error rows, exact node/edge membership, old/new FTS search results and revision. Reset checks identity, keys, failures, native vector bytes and schema; main-cache state and corpus/config bytes must remain unchanged. A pinned WAL reader sees its old snapshot during reindex.

Concurrency schedules use pipe handshakes, including a checkpoint reached only when a nonblocking **real flock** reports contention. They cover two edit/commit orderings, duplicate deltas, killed lock owners, reindex versus publication, and competing config resets. No scheduling sleeps are used; a short polling interval only bounds child exit checks. Parent control waits and exit waits have 20-second deadlines; Drop kills/reaps surviving children. SQLite busy waits are bounded at 10 seconds after opening.

## Fixture boundaries

The first line is `<!-- {"links":[["target.md","includes"]]} -->`, followed by searchable text. One fixture file represents one node; only `rel` and `includes` are accepted. Corrupt JSON is invalid input. This deliberately small grammar is **not X-4 parsing**: section anchors, full metadata, source identities, Git snapshots and Markdown syntax belong to TK-7/TK-8/TK-10. Symlinks are skipped in these temporary fixtures; production traversal policy is not implemented.

Change detection uses real inode/size/mtime and SHA-256. Files within a conservative **one-second timestamp tick** of their recorded check are always hashed. The same-tick test seeds an old real mtime and explicitly normalizes the scratch database's recorded check clock into that tick, asserting the racy precondition independently of initialization/scheduling delays. It then restores the exact mtime and verifies equal inode/size before checking the new search result. Filesystems with coarser-than-one-second timestamps and edits that deliberately restore an old, non-racy timestamp are not covered. A metadata change during a file read returns an error; arbitrary concurrent in-place file writes are not an atomic snapshot.

Config fixtures contain provider/model strings, an empty parameters object and integer max_retries. Identity hashes sorted JSON for that subset; this is **not a general JCS implementation** or full config-schema validation. Config reset uses native vec1, but ATTACH, dimension changes with old readers, embedding/drain correctness and stale embedding results belong to TK-3.

These are **process-death** checks at deterministic application checkpoints, not physical power-loss or instructions sampled inside SQLite COMMIT. Device/filesystem durability (TK-38), performance, x86-64, full schema migrations and production integration remain unassessed.

See [TK-2](../../docs/zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-2-durability-harness.md) and the [execution report](../../docs/zkit/letopis/checks/TK-2/report.md).

The checked-in [schema.sql](schema.sql) is a byte-for-byte snapshot of the local D-93/D-95 physical design used for this proof. The design revision remains a separate uncommitted change; this crate compiles from a clean checkout without including that mutable document. It is a scratch schema, not a production migration.
