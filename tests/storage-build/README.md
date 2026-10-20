# TK-1: storage build compatibility

Retained integration checks for the selected Rust / rusqlite / SQLite / native vec1 build. This is a standalone test package, not the production storage adapter. No existing project or research database is used.

## Pinned inputs

| Input | Pin |
|---|---|
| Rust | `1.95.0`, including rustfmt and Clippy (`rust-toolchain.toml`) |
| rusqlite | `0.40.2`, `load_extension` and default `bundled` feature |
| libsqlite3-sys / bundled SQLite | `0.38.2` / `3.53.2`; resolved dependencies in `Cargo.lock` |
| vec1 | `0.8`, SQLite commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7` |
| vec1 C SHA-256 | `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9` |
| Linux base | `rust:1.95.0-bookworm`, immutable digest in `Dockerfile` |

The runner downloads the [pinned upstream vec1 source](https://github.com/sqlite/sqlite/blob/a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7/ext/vec1/vec1.c) into ignored `.build/`, verifies SHA-256 before compilation, and compiles it against the headers shipped by the **same resolved libsqlite3-sys** that builds SQLite. Generated native libraries live in unique temporary directories, never reused across platforms. Flags: `-O2 -DNDEBUG -fPIC`, `-dynamiclib` on macOS / `-shared` on Linux, `-pthread -lm`; no host-specific `-march` or mandatory AVX2.

## macOS or Linux host

Requires Python 3, rustup, and a platform C compiler. The first run needs network access for the pinned Rust components, Cargo dependencies, and vec1 source. From the repository root:

```sh
python3 -B tests/storage-build/run.py
```

It runs source-integrity tests, builds vec1, runs all Rust tests, exercises capability-rejection examples, checks formatting, and runs Clippy with warnings denied. Logs and a manifest go into a new `.build/evidence-<UTC timestamp>/` directory. An explicit destination must not exist:

```sh
python3 -B tests/storage-build/run.py --evidence /tmp/tk1-host-evidence
```

The manifest records the tested sources, lockfile and SQLite header hashes, native-library hash, tool versions, commands, selected environment values, log names, exit codes, and final result. A failed check returns nonzero and preserves its logs. Cached source corruption is rejected; repair it by obtaining the pinned source again, never by changing the expected digest.

## Linux ARM64 in Docker

From the repository root, using an ARM64 Docker engine:

```sh
docker build --platform linux/arm64 -t letopis-tk1:rust-1.95.0 tests/storage-build
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD/tests/storage-build,target=/checks" \
  letopis-tk1:rust-1.95.0
```

The default container command additionally builds against Debian's old system SQLite (`--system-sqlite`) and verifies its rejection. An explicit persisted evidence directory can be mounted separately:

```sh
mkdir -p /tmp/tk1-evidence
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD/tests/storage-build,target=/checks" \
  --mount type=bind,source=/tmp/tk1-evidence,target=/evidence \
  letopis-tk1:rust-1.95.0 \
  python3 -B run.py --evidence /evidence/linux-arm64 --system-sqlite
```

Debian packages provide Python, the C compiler and an intentionally unsupported system SQLite for the negative fixture. Their actual versions are recorded; this is not a claim of a bit-identical apt environment. Normal checks use bundled SQLite, not that system library. Cargo targets are separated by OS/architecture, temporary databases by run/test. Evidence destinations cannot be overwritten.

## What is checked

- SQLite `3.51.3+`, WAL, `synchronous=FULL`, foreign keys, macOS `fullfsync`.
- FTS5 contentless-delete read/search/delete, committed persistence, and insert/update/delete rollback both explicitly and when the transaction is dropped.
- Native vec1 registration on each connection, stored float32 bytes, no-index/cos search, committed update/delete/reopen, dimension rejection, and explicit/drop rollback.
- FTS and vector writes commit or roll back together **inside one database**.
- Extension loading is disabled again after registration; missing/invalid extensions fail with a diagnostic.
- Real SQLite rebuilt without FTS5 is rejected. The version boundary `3.51.2` vs `3.51.3` is tested, and Linux also rejects real system SQLite `3.40.1`.
- Missing `LETOPIS_VEC1_EXTENSION` fails tests rather than skipping coverage. Use the runner to supply its freshly compiled library; never point it at an untrusted native library.

These checks demonstrate build compatibility on tested macOS ARM64 and Linux ARM64, not full NFR-5/NFR-7 compliance. Cross-database ATTACH, config reset and old read views belong to TK-3; production storage to TK-9; physical power loss to TK-38. x86-64, embedding providers, load and performance are not assessed here.

See the [task](../../docs/zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-1-build.md) and [execution report](../../docs/zkit/letopis/checks/TK-1/report.md).
