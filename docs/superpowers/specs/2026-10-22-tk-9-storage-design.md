# TK-9 — SQLite storage, static vec1, and responsibility-based modules

> **Date:** 2026-10-22
> **Status:** implemented and verified on 2026-10-22; all task, whole-branch and final scoped reviews complete.
> **Branch:** TK-9, based on local main `5869ffd6e7fb148f79c5eec2f1c7856913c662fd` (includes D-98).

## 1. Intent and design authority

Implement the production storage foundation required by the thin slice: the agreed cache schema, vectors.db, typed application interfaces, and statically linked vec1. Subsequent services must be able to use it without knowing SQLite. Keep the design synchronous, small, and explicit.

The user approved one Storage interface, one concrete SqliteStorage implementation, private modules by responsibility, inward dependencies, and lib.rs wiring. Later refinements remove Port/Adapter from type names and directory names, remove contracts/, place interfaces in mod.rs, and put each concrete implementation with its struct in a separate file. These naming/file decisions are recorded in [D-99](../../zkit/letopis/decisions.md#D-99). The user subsequently approved this written specification for planning on 2026-10-22. The user subsequently approved the written plan and selected Subagent-driven execution; that gate is complete.

Sources, in precedence order:

- [TK-9 task](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-9-storage.md).
- [Program structure](../../zkit/letopis/architecture.md#program-structure) and [structure review](../../zkit/letopis/architecture.md#program-structure-review).
- [D-95](../../zkit/letopis/decisions.md#D-95), [D-96](../../zkit/letopis/decisions.md#D-96), [D-97](../../zkit/letopis/decisions.md#D-97), [D-98](../../zkit/letopis/decisions.md#D-98), and D-99.
- [Current DDL](../../zkit/letopis/mechanics/storage-schema.sql), [mechanics](../../zkit/letopis/mechanics.md), and [data model](../../zkit/letopis/data.md).
- [I-2 projection ownership](../../zkit/letopis/contracts/I-2-projections.md), [I-8 reads](../../zkit/letopis/contracts/I-8-read-view.md), [I-11 problems](../../zkit/letopis/contracts/I-11-diagnostics.md), and [X-1 errors](../../zkit/letopis/contracts/X-1-cli.md).

The historical storage-model sections explicitly superseded by D-91 through D-96 do not reintroduce jobs, errors/history tables, recovery records, runtime extension loading, or corpus writes. The product crate, pinned Rust 1.95.0/edition 2024, parser, and Git snapshots already exist; reuse them.

## 2. Scope and handoff

TK-9 owns connection/bootstrap policy, executable schemas, primitive graph/source/file/FTS operations, primitive vector/failure/query-error operations, atomic units, safe errors, static vec1, composition, and tests through the application interface.

It also owns the mechanical migration of existing product Git/Markdown modules needed to apply D-99 consistently. Rename GitSnapshotPort to GitSnapshots and GitCliAdapter to GitCli, move their source files, and update live consumers. Preserve behavior and visibility; do not add legacy aliases or a new wrapper service. Existing historical evidence continues to describe its captured code paths.

The following remain in their roadmap tasks:

- TK-10 prepares complete per-file projections, validates them, detects corpus changes, holds index.lock, handles invalid files, and orchestrates publication/source reconciliation.
- TK-11 supplies CLI handlers, filesystem/runtime integration, configuration loading, and lock ownership. There is no placeholder main.rs in TK-9.
- TK-12 implements full IndexReadView/config selection, search query preparation, ranking, traversal, and response policies.
- TK-13/TK-27 implement provider calls, drain selection/retries, configuration validation/JCS, and lifecycle orchestration. Storage supplies their atomic primitives; it does not start a worker or invoke HTTP.
- Later gates own resource/quality measurements, physical power-loss drills, and x86_64 release validation.

## 3. Physical structure and dependency direction

```text
src/
  domain/                 # Existing pure types/rules; no I/O dependencies
  service/
    mod.rs                # Application-owned traits and module API
    error.rs              # Typed storage error and its implementations
    model.rs              # Related storage request/result values
  git/
    mod.rs                # Child declarations and necessary exports
    cli.rs                # GitCli struct, inherent methods, GitSnapshots impl
    process.rs            # GitProcess and its implementation
    objects.rs            # Existing object-verification implementation
    refs.rs               # Existing ref-retention implementation
  markdown/
    mod.rs                # Public parsing API and module declarations
    parser.rs             # Parsing flow moved out of mod.rs
    diagnostic.rs         # Diagnostic type and its implementation
    metadata.rs
    links.rs
    scan.rs
    search.rs
  storage/
    mod.rs                # Child declarations and necessary exports
    sqlite.rs             # SqliteStorage struct, constructors, Storage impl
    cache_write.rs        # CacheWrite implementation with its struct
    vector_write.rs       # VectorWrite implementation with its struct
    read.rs               # Scoped storage reads with their concrete handle
    connection.rs         # Connection policy
    schema.rs             # Schema bootstrap
    graph.rs              # SQL graph/source operations
    files.rs              # SQL file-state operations
    fts.rs                # SQL FTS operations
    vectors.rs            # SQL vector/failure/query-error operations
    native.rs             # Narrow static vec1 registration boundary
  lib.rs                  # Actual constructor calls for available components
```

This is the intended decomposition, not an instruction to create empty files. Cohesive private SQL helpers may be adjusted in the implementation plan without changing ownership. Interface definitions belong to service/mod.rs; related data/error definitions may live in private files and be explicitly reexported. No ports/, adapters/, or contracts/ directory layer is introduced. Every concrete implementation stays with its struct in a separate file: git/cli.rs contains GitCli and its impl blocks; storage/sqlite.rs contains SqliteStorage and its impl blocks. mod.rs contains no concrete implementation bodies. The existing Markdown parse API is defined in parser.rs and reexported by mod.rs; no unused Markdown trait or artificial parser struct is introduced.

Application code imports Storage/GitSnapshots from service. Concrete Git and storage modules implement those traits and may use domain types; they never orchestrate one another. SQL/SQLite/vec1 types stay inside storage. Domain has no dependency on service, SQLite, Git processes, files, HTTP, or CLI. lib.rs calls constructors, returning only the interfaces needed by callers; no registry, global service locator, DI framework, pool, async runtime, or forwarding service is added.

When actual services appear, each service implementation also receives its own file with its struct and impl blocks. No future CLI, embedding, filesystem, or service skeletons are created here.

## 4. Application interface and transaction ownership

One Storage trait exposes related operations through typed request/result values and scoped read/write interfaces. A cache write unit and a vector write unit are owned capabilities, not independent service-level storage components. Use associated types/lifetimes to keep concrete SQLite handles private. Do not expose rusqlite Connection/Transaction/Row, arbitrary SQL strings, or callbacks that perform Git/HTTP through the application interface.

Small interfaces can be defined directly in service/mod.rs. Split them into private responsibility-named files only when their actual size warrants it, reexporting the same short public names. Final method signatures are enumerated in the implementation plan before coding; they must express the operations and ownership specified here rather than generic SQL access.

Required operation families:

| Family | Input/output responsibility |
|---|---|
| Cache read | Read revision, node/source metadata, declared edges, file state, and FTS matches through typed values |
| Cache write | Begin one unit; insert/update/delete projections and FTS/file state; advance revision; explicit commit |
| Vector metadata/read | Read config identity and vector/failure/query-error state, lookup by text hash, and verify native vec1 round-trip/nearest operations |
| Vector write | Reserve attempts, store success, record failure, replace/clear query error, reset configuration; each action checks its captured identity |
| Scoped reads | Keep the connection/transaction exclusive to its owner; release with RAII; expose observed revision/config identity without substituting later global state |

The caller owns a cache unit and commits explicitly. Operation methods join that unit, without nested BEGIN/COMMIT. An uncommitted unit rolls back when dropped, including after an operation returns an error. The publication service will hold index.lock and validate its prepared input in TK-10; TK-9 does not pretend a process-local Rust value proves a kernel lock.

A cache unit changes only the cache: nodes/edges, contentless FTS, files, and project.revision. Vector writes use a dedicated vectors.db connection, so BEGIN IMMEDIATE there does not reserve a writer lock on the attached cache. There is no combined cache-publication/vector-reset commit and no cross-file atomicity claim.

TK-9 supplies storage-level scoped reads needed to test these primitives, not the full TK-12 IndexReadView with configuration-file policy and ranking. SQLite may establish snapshots of attached databases at different times. A vector operation must verify the identity observed in its own read transaction against the identity captured by its caller before returning usable vectors; a mismatch must not return a vector for the wrong model. TK-12 owns the FTS-only response policy. Do not describe the two database files as a global instantaneous snapshot.

## 5. Cache schema and primitive behavior

Use the current DDL as the normative schema. Product SQL files live with storage and are executable bootstrap inputs, not a second divergent model. Keep a schema parity check against the canonical design. The retained check is `tests/storage/check_schema.py`, run by each source-built platform acceptance runner; its regression changes a disposable column type and requires rejection. It compares the fixed DDL fixtures with comments and the intentional project seed excluded, without a general SQL parser. Initial schema setup is transactional per database and safe to repeat; reopen never recreates valid tables or silently destroys an unsupported/damaged schema. Surface a safe failure so later maintenance commands can rebuild deliberately.

- project: singleton revision counter, initialized once; checked integer conversion and overflow handling.
- nodes: unique canonical ID, stable row_id on UPSERT, agreed kind/URI/hash/time/source snapshot/line/search-hash constraints. Knowledge nodes carry searchable hashes; source nodes carry no search_hash/source_snapshot. Source ID equals normalized URI and missing source bytes may have hash=NULL.
- edges: unique source/target/type triple, FK only on source, no self-loop. Missing targets remain declared; do not introduce an active flag or target FK. Incoming edges survive removal of their target. Delete a removed node's outgoing edges before the node.
- text_index: FTS5 contentless-delete, linked by the stable node row_id. Replace/delete index entries in the same cache unit as their metadata. No stored document body or second shadow text column.
- files: one row for every Markdown candidate selected by D-97, including unmarked files. Keep size/mtime/inode/hash/checked time and the validity/error CHECK. Store safe errors without raw document excerpts. TK-10 decides whether to retain a previous valid projection for an invalid current file.

Source projection primitives must represent normalized source identity, missing bytes, source creation time derived from current referring knowledge, and removal after its last external reference. The later publication service computes the complete valid projection; storage persists the supplied projection atomically, rather than calling Git or reparsing Markdown.

Use parameters for values. Convert Rust unsigned values to SQLite integers with checked conversions; a value outside the supported range is an explicit error, never truncation/wrapping. Global graph/domain validation remains in pure Rust rules and the publishing service. No jobs, error-history, lock-owner, recovery, or reindex-state tables are added.

## 6. Vector database and identity guards

vectors.db keeps meta, vector_keys, native vector_index, query_error, and embed_failures. Preserve the agreed schema and no-index/cos baseline. Meta is absent before model configuration; do not invent a default provider/model/dimension. Low-level vector methods receive the already-validated config identity and expected dimension from their caller; configuration parsing and JCS do not enter TK-9.

Success uses one vectors.db transaction: recheck meta identity, add the text-hash key and vector together or reuse the existing key, and remove its failure row. Duplicate text hashes share one vector. Never commit an orphan key or a vector associated with a different key. A result carries its originally captured search_hash; current node association is always by the node's current search_hash, never by a node ID alone.

Attempt reservation increments embed_failures before the future provider call. A crash may consume a reserved-but-unsent attempt; restart does not reset it. Failure recording also checks identity, so an old request cannot poison a new configuration. Validate nonempty, finite f32 vectors and their dimension before committing them. A dimension mismatch or invalid vector cannot leave a partial key/vector pair. Exhaustion policy/retry timing belongs to the embedding service; the primitive receives the applicable budget and reports whether it reserved an attempt.

Reset uses a dedicated vectors.db BEGIN IMMEDIATE transaction, rechecks identity, recreates native vector_index to clear the previous dimension, clears keys and embedding failures belonging to the old configuration, and writes the new identity. The singleton query error follows its separate X-1 last-failure/until-success rule; do not add a config-reset clear policy. On rollback, the previous committed identity and vectors remain. An unchanged identity is a no-op. Cache revision does not advance on vector writes/reset. Removing/rebuilding only the cache does not erase vectors.db.

The singleton query error is replaced on failure and cleared on a successful query embedding. It contains only a safe message and timestamp. TK-9 does not fetch embeddings, schedule retries, launch a drain, reclaim concurrent orphan vectors, or implement final search ordering.

## 7. Connection policy and static native build

Use the already agreed stack:

- Rust 1.95.0, edition 2024, existing lint policy.
- rusqlite `=0.40.2`, default features disabled, bundled SQLite; product load_extension feature disabled.
- Build dependency cc `=1.9.0`, the version already resolved by the TK-1 lockfile. Pin resolved dependencies in the product lockfile.
- Use existing sha2 for hashes/checksum verification. No new runtime dependency is needed just to name or organize components.

Each applicable connection enables and verifies foreign_keys, WAL, synchronous=FULL, and macOS fullfsync. Configure both writable database files, not only the primary connection. Require SQLite >=3.51.3 and FTS5 contentless-delete/native vec1 capability. Do not fall back to system SQLite or silently degrade the stored schema. SQLite busy, corruption, and I/O failures remain explicit errors; no new timeout knob or hidden retry loop is introduced here.

Vendor the original vec1 C source and provenance/license information. Source: SQLite commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`, ext/vec1/vec1.c, version 0.8, SHA-256 `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`, matching [TK-1 runner](../../../tests/storage-build/run.py). build.rs verifies the source checksum, compiles it with cc against headers matching bundled SQLite, and links the resulting archive into the product. The exact integration flags and SQLite header plumbing are settled and tested in the implementation plan; do not patch the pinned C source to make the checksum check meaningless.

Register the statically linked extension through sqlite3_auto_extension before opening product connections. Keep this narrow unsafe FFI boundary private, documented, and checked for registration errors. Registration must be safe when multiple Storage instances open in one process. Do not accept a runtime extension path, enable SQL load_extension, download at runtime/build-script execution, or depend on a loose .so/.dylib. Prepare vendored sources/Cargo dependencies explicitly before locked offline verification.

## 8. Errors, privacy, and composition

Use a typed StorageError with finite classifications suitable for the existing X-1 error catalog: invalid_request, not_found where applicable, and internal_error for SQLite/I/O/integrity failures. An identity change is an expected typed outcome for vector operations; it is not a new CLI conflict error. The CLI mapping itself belongs to TK-11.

Public messages are fixed safe context. Retain useful internal causes through Error::source, but do not print SQL parameters, document text, provider payloads, credentials, or arbitrary raw SQLite messages through the public Display/message path. Related failure records supplied by services must already be safe. No broad generic error framework or table of error history is introduced.

lib.rs exposes actual construction of SqliteStorage through the Storage interface, alongside the renamed GitSnapshots construction. Database paths belong to construction, not domain rules; the CLI chooses the project's paths in TK-11. A small executable consumer used for static-link validation can call this constructor without introducing a product CLI. Keep helpers private, public fallible APIs documented, methods named with verbs, functions around 40 lines and at most 60, and tests in Arrange/Act/Assert form.

## 9. Verification and acceptance evidence

Run new behavior tests against the product implementation through service interfaces and lib.rs construction. SQL-level schema/capability inspection is allowed inside storage tests to verify things the public interface should not expose. Use fresh temporary databases; no user corpus or existing cache is a fixture.

| Task criterion | Required evidence |
|---|---|
| Tables, keys, CHECKs and operations through interfaces | Round-trip knowledge/source/edges/files; reject duplicate keys, self-loops, invalid line/source/file states and numeric overflow; show a dangling target remains declared and becomes joinable when added; stable row_id across updates |
| Atomic units | Force a late error and separately drop an uncommitted unit; fresh connections see no partial graph/FTS/file/revision change. Successful units publish their complete values. Test key/vector/failure success and rollback atomically, identity changes at reservation/success/failure, and persistence after close/reopen |
| WAL/FULL/fullfsync/FK | Read back PRAGMAs from applicable connections for cache and vectors.db; macOS-only fullfsync result labeled by platform |
| Native vec1 in one executable | Check pinned C checksum and native version; round-trip and nearest query; reject empty/nonfinite/wrong-dimension vectors; config reset/rollback across dimensions; execute a copied product-linked validation binary with no extension library alongside it and confirm extension loading is disabled |
| No raw document/source copies | Inspect schema for absence of text-body tables/columns; contentless FTS cannot return indexed source text; exercise search with a unique text marker and confirm only metadata/matches are exposed. FTS token/posting data is expected and is not claimed absent from database bytes |
| Cache/vector independence | Rebuild/remove only the cache fixture and reopen with the existing vectors.db; unchanged text hashes remain reusable and failures remain durable; vector/reset operations do not advance cache revision |
| D-99 and hexagonal | Check real source files, imports/visibility, each struct with its own implementation file, service-owned interfaces, private SQL/FFI details, and actual constructor calls in lib.rs. Verify old live module/type names are gone while historical evidence is untouched |

Preserve the existing 53 test scenarios during the mechanical module/type migration. Run ./scripts/check-rust.sh and strict Rustdoc after implementation, with the actual revised product dependencies prepared and locked. Do not add behavioral tests merely to mirror a path/name change. New tests verify storage behavior and failure boundaries.

Run the product/static-link checks on macOS ARM64 and Linux glibc ARM64 with platform/toolchain/native-source/input hashes and logs saved under a new TK-9 checks directory. Use a source build for Linux and run the resulting validation without network or extension-library files. Report exact commands/counts after execution. x86_64, physical power loss, minimum old Git execution, resource capacity and real model quality remain unverified unless independently exercised; do not infer them from these checks.

## 10. Review and implementation handoff

A fresh pre-change baseline on the branch passed ./scripts/check-rust.sh: formatting, compiler checks, Clippy, and 53 root tests. This is context for the existing tree, not evidence that TK-9 storage already exists.

This specification records the agreed intent, module/file refinements, and concrete storage behavior. The user approved the written specification with “go on” on 2026-10-22. The baseline was captured before implementation; all six implementation tasks are now complete. The [implementation plan](../plans/2026-10-22-tk-9-storage.md) enumerates signatures, cohesive tasks, exact test commands, static-build plumbing, and evidence locations. The user approved that plan and chose Subagent-driven execution before implementation began.

## Implementation evidence (2026-10-22)

Implementation and platform validation are recorded in the [TK-9 report](../../zkit/letopis/checks/TK-9/report.md): 106 Rust tests and eight Python checks per platform, strict Rustdoc, canonical-DDL parity with deliberate drift rejection, copied native executable and two disposable-source negative controls. Current runner-cleanup source is `c93a2f9d795da1ea211e3ef5e7cdb939447aa255`; original implementation review source was `60fc1f0cd9f4be62c62d486632cd52446a9c46a0`; earlier receipts remain historical. All six independent task gates, broad final review and the single fix-wave re-review passed; all four findings are addressed. Engram outcome is recorded at revision 77. Later documentation commits are not claimed as executed. Integration and later roadmap scopes remain separate.


### Final-review evidence clarification

Native receipts observe SQLite 3.53.2 and vec1 `version 0.8 (NEON, multi-threaded)`;
assertions require SQLite >=3.51.3, vec1's version 0.8 prefix, FTS capabilities and
the connection policy. Exact patch/NEON/thread-description strings are not asserted.
The compact final summary is [verification.json](../../zkit/letopis/checks/TK-9/verification.json). Raw receipts are retained outside the tracked tree; the report records their archive checksum and original Git revision.
