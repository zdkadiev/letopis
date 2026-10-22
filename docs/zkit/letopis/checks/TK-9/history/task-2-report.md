# Task 2 implementation report

Status: DONE. Commit: `422b34b` (`feat: link vec1 statically and initialize SQLite storage`). Base: `342e06ac93a2809f726f64f180d9a463624363fd`, branch TK-9. The parent-owned dirty specification/plan/task-card edits were preserved; no Engram changes were made.

## Implemented

- Static, checksum-verified upstream vec1 0.8 compilation. `build.rs` reads the direct `libsqlite3-sys` dependency's `DEP_SQLITE3_INCLUDE`; cc receives SQLITE_CORE and sqlite3_extension_init=letopis_vec1_init. No patched C, downloads, runtime loading, SIMD overrides, target-native flags, or generated output outside OUT_DIR. Linux links m/pthread.
- Typed three-argument static ABI registration with OnceLock of the checked SQLite result, no transmute or extension-list reset. Product and temporary schema-reference connections register before opening.
- Exact pinned dependencies and one bundled SQLite engine. Fixed safe public StorageError messages retain internal Error::source causes.
- Paths resolve before any bootstrap; absolute literal UTF-8 disk filenames, NUL/missing-parent rejection, canonical path and Unix device/inode alias detection. READ_WRITE|CREATE|NO_MUTEX without URI; ATTACH uses a bound filename.
- Explicit zero busy timeout; WAL/FULL/FK and macOS fullfsync set and verified on both files and attached vector read connection; version/native/FTS capability checks.
- Per-file BEGIN IMMEDIATE bootstrap rechecks user_version/schema under the lock. Empty version zero initializes once; version one recognizes exact named definitions through an in-memory reference using the same SQL, excluding SQLite shadow/internal objects. Unknown, partial, altered and extra-user layouts fail without recreating data.
- SqliteStorage retains only its attached read connection. Scoped SqliteRead owns an exclusive rusqlite transaction and pins decoded revision/config identity. No SQL/Connection/Deref/writes in the application interface. `open_storage` constructs the concrete implementation with precise `use<>` capture.
- Sha256 moved into its own pure domain file, preserving existing hashing/hex implementation, with exact lowercase parse and InvalidHash.
- Dockerfile copies build.rs/vendor before Cargo commands and all root integration test files via wildcard.

## RED and iteration evidence

Before adding product code/dependencies, `cargo test --locked --offline --test storage_bootstrap` failed with E0432 because open_storage, Storage, StorageRead and StorageErrorKind were absent. Captured actual output:

```text
   Compiling letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
error[E0432]: unresolved import `letopis::open_storage`
 --> tests/storage_bootstrap.rs:1:5
  |
1 | use letopis::open_storage;
  |     ^^^^^^^^^^^^^^^^^^^^^ no `open_storage` in the root

error[E0432]: unresolved imports `letopis::service::Storage`, `letopis::service::StorageErrorKind`, `letopis::service::StorageRead`
 --> tests/storage_bootstrap.rs:2:24
  |
2 | use letopis::service::{Storage, StorageErrorKind, StorageRead};
  |                        ^^^^^^^  ^^^^^^^^^^^^^^^^  ^^^^^^^^^^^ no `StorageRead` in `service`
  |                        |        |
  |                        |        no `StorageErrorKind` in `service`
  |                        no `Storage` in `service`

For more information about this error, try `rustc --explain E0432`.
error: could not compile `letopis` (test "storage_bootstrap") due to 2 previous errors
```

First pinned native build succeeded. First focused run passed ten bootstrap tests. A targeted adversarial schema test then found a real implementation bug: SQL LIKE 'sqlite_%' treated underscore as a wildcard and hid the extra user table sqliteXprivate. `cargo test --locked --offline --test storage_bootstrap sqlite_like_prefix_does_not_hide_extra_user_tables` returned 101, one failed test, with `invalid storage accepted`. Changed to literal-prefix GLOB 'sqlite_*'; the test passed with the full focused file.

Intermediate `cargo test --locked --offline storage::` initially exposed a missing ValueError import in the added domain test (E0433), corrected before final validation. Clippy iterations reported doc_markdown for SQLite and manual_let_else in three test error branches; corrected narrowly, no lint suppressions.

## Final checks and actual results

| Command | Actual result |
|---|---|
| cargo generate-lockfile | Success; existing locked versions preserved, required dependency closure added |
| cargo fetch --locked | Success |
| cargo test --locked --offline --test storage_bootstrap | 14 passed, zero failed |
| cargo test --locked --offline storage:: | Two private native/connection tests passed; other targets filtered |
| ./scripts/check-rust.sh | Success: formatting, all-target compiler check, Clippy -D warnings, all 70 root scenarios passed; zero failures, zero ignored; doc tests zero |
| RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps | Success, no documentation warnings |
| cargo tree --locked --offline -e features | One libsqlite3-sys 0.38.2, bundled enabled; rusqlite/load_extension absent |
| cargo tree --locked --offline -i libsqlite3-sys | One engine with direct root and rusqlite consumers |
| git diff --check | Success on then-tracked unstaged changes (untracked vendor source was not covered) |
| git diff --cached --check | Reports upstream trailing whitespace and terminal blank line only in unchanged vendor/vec1/vec1.c; intentionally retained to preserve the mandated byte-for-byte SHA |

The original 53 scenarios remain; 17 new scenarios are one domain hash test, two private native/policy tests and fourteen bootstrap tests. The final root check includes the later missing/altered index and altered FTS definition cases.

Full final check, Rustdoc, features and initial RED logs are available in `/tmp/tk9-task2-check-rust.log`, `/tmp/tk9-task2-rustdoc.log`, `/tmp/tk9-task2-features.log`, `/tmp/tk9-task2-red.log` respectively. These are local temporary logs, not historical proof artifacts.

## Native pins and platform

- Host: Darwin arm64. rustc 1.95.0 (59807616e 2026-04-14), cargo 1.95.0 (f2d3ce0bd 2026-03-21).
- rusqlite =0.40.2 (default features disabled, bundled); direct libsqlite3-sys =0.38.2 (default features disabled, bundled); cc =1.9.0; sha2 =0.10.9 (normal/build); tempfile =3.23.0 (dev).
- The rusqlite transitive sys dependency enables its own default/min-version metadata features; Cargo still resolves exactly one bundled 0.38.2 engine. No rusqlite/load_extension feature.
- Matching bundled sqlite3.h reports SQLite 3.53.2. Runtime tests verify SQLite >=3.51.3, vec1_info() starts with `version 0.8`, contentless-delete works, and SQL load_extension returns not authorized.
- Pinned unchanged C: SQLite commit a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7, ext/vec1/vec1.c, SHA-256 a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9. Copied cached bytes after hashlib verification; build independently verifies the same digest. Staged bytes are checked again immediately before commit.
- Actual macOS fullfsync=1, journal_mode=wal, synchronous=2, foreign_keys=1, busy_timeout=0 are read back by private tests for both writable files and the attached read connection.
- Eight threads each open separate Storage/SQLite instances and observe native vec1. Two barrier-synchronized initializers either complete or return typed InternalError; subsequent reopen is a complete revision-zero schema. Held BEGIN IMMEDIATE yields an internally caused SQLITE_BUSY StorageError; releasing it allows reopening.

## Files owned by this task

Cargo.toml, Cargo.lock, build.rs; vendor/vec1/{vec1.c,README.md}; src/domain/{mod.rs,value.rs,sha256.rs}; src/service/{mod.rs,error.rs}; src/storage/{mod.rs,native.rs,connection.rs,schema.rs,sqlite.rs,read.rs,cache.sql,vectors.sql}; src/lib.rs; tests/domain_values.rs; tests/storage_bootstrap.rs; tests/support/storage.rs; tests/git/Dockerfile.

## Self-review

Reviewed physical files, visibility, dependency direction and constructor calls. Domain stays pure. Service owns the finite traits/error vocabulary and imports no adapters. Concrete storage struct and trait implementations stay in sqlite.rs/read.rs, native FFI and SQL stay private, mod.rs declares/reexports only. lib.rs performs actual construction. Writer connections are constructor locals and no future-task stubs are present. Read capability holds its transaction and exclusive borrow; owned-storage lifetime regression compiles and runs. SQL filenames are bound; schema-only format strings use private main/vec literals. Rejected aliases retain exact existing fixture database bytes. Refused schemas retain existing project/failure/raw_text rows; no migration/drop/recreate fallback exists. Native C is unchanged. Focused functions/tests stay within the project's length limit and selected lints pass.

## Limits

This task's native execution is macOS ARM64 only. Updated Dockerfile has not been built/run here; Linux glibc ARM64, copied-binary isolation checks and later vector operations remain the designated later task/platform runner work. No x86_64, power-loss drill, historical standalone proof harness, provider/model evaluation, or performance claim is made. Missing parent directories are rejected; TK-11 owns parent creation/permissions. No merge/push/issue changes.
