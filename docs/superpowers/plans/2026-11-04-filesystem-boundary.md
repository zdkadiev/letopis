# Filesystem-only interfaces implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Apply the user-approved generic filesystem boundary and entity-first naming to the existing rafactor implementation without changing CLI/storage behavior.

**Architecture:** One LocalFileSystem implements small application-owned file interfaces. Consumers select filenames/roles and map file errors; storage retains SQLite, closure owners and transactions. Generic file observations protect I/O around database opening in composition. Existing retry/time dependencies are extracted from filesystem where actually required.

**Tech Stack:** Pinned synchronous Rust/rusqlite/rustix, existing mock/test facilities; no new dependency/runtime.

**Spec:** [Filesystem-only component boundary](../specs/2026-11-04-filesystem-boundary-design.md).

**Status:** User authorized implementation on rafactor, then explicitly requested moving all work into the primary checkout. Active directory is /Users/zdkadiev/Repos/letopis, branch rafactor; initial base b40b4f6 and guidance commit aba8d4f. Source changes, staging and ignored execution records were copied and verified. The app-protected filesystem-refactor worktree is inactive and must not be merged back later. Baseline lib tests:490 passed/0failed/9ignored. No push/main merge/PR update is authorized.

## Global Constraints

- One production filesystem component LocalFileSystem; no DatabasePaths or replacement active context wrapper. Real metadata values and RAII resources may remain.
- Filesystem production interfaces/types/errors/filenames contain only file/filesystem concepts; no database/cache/vector/configuration/embedding/knowledge/command roles or SQLite handles. Consumers own application-specific paths and interpretation.
- Filesystem operations/traits use entity_action_by_filter_and_filter; predicates use entity_is_condition, especially file_is_exist. Result types reflect actual fallibility; never hide I/O/unsafe-path failures as false.
- Preserve public CLI JSON/diagnostics, config/lock/init semantics, no-follow/identity/race/alias protection, durable no-clobber publication, read-only status and stable document exclusions. New APIs must distinguish absent/nonregular/unsafe/failing I/O where callers require it.
- Preserve CacheDatabase/VectorDatabase closure transactions, query semantics, schemas/version numbers, vector state, background protocol and synchronous bounded drain. Do not import issue90 schema/key changes or alter its PRs.
- Generic database-file identity/existence checks inspect metadata, never database bodies. Per-operation file state is short-lived; pinned descriptors preserve real directory/lock identities without unbounded per-document caches.
- Application-owned traits/types/errors stay in service/mod.rs/types.rs/error.rs. Implementations stay in their existing technology owners and lib.rs remains actual composition. Follow current rafactor AGENTS including table-first SQL and file-first I/O; no pass-through service or speculative interface layers.
- Transitional legacy APIs may remain only in Task1 while consumers still compile; Task2 removes them entirely. No production default-unimplemented stubs or final compatibility aliases.
- Read-only and writable paths retain their existing capabilities. Locks remain persistent and observation never creates/writes owner state; actual acquisition may create the caller-selected lock/parent safely.
- Exact new receipts go to new ignored artifact directories. No paid/live-provider calls, real user-cache writes or historical evidence rewriting.

## Review Focus

1. A document symlink/nonregular entry remains an excluded/missing document, while config/database callers still reject it rather than treating unsafe state as missing.
2. A private directory or existing file is replaced between generic verification and SQLite open/attach: detect it without moving file policy back into storage.
3. Metadata checks during legitimate database writes do not compare content size/mtime as file identity; hardlink aliases and replaced inode/device still fail.
4. Status/config observation on an uninitialized checkout creates nothing; no-clobber init keeps another writer's complete winner and synchronizes durable state.
5. Shared filesystem mocks, retry clocks, busy locks and PID observations retain exact caller semantics after role-specific interfaces disappear.

## File responsibilities

- service/mod.rs,types.rs,error.rs: generic file interfaces, pure file metadata/resource values, typed file failures; later remove old FileSystem/EmbeddingFiles/StatusFiles/InitFiles/EmbedLocker role contracts.
- filesystem/local.rs + existing low-level confined/read helpers: LocalFileSystem and generic file API implementations. Small private path/write/lock helper files may own actual low-level protocols/RAII resources; no new component wrapper or per-consumer module. Rename/remove filesystem/database.rs, init.rs/status.rs as their application roles disappear; keep implementation ownership/readability clear.
- service consumers: document_refresher, embedding, configuration, status, init, background_runner, inspection, provenance, source, search/traversal as needed; migrate tests/doubles alongside them. Application paths/constants remain outside filesystem.
- process: concrete synchronous timing implementation only for existing actual Clock use, if required; no new runtime/scheduler.
- lib.rs and storage/connection tests: real generic-file wiring around open/attach, no DatabasePaths/prepare_paths role helpers; public fixture path behavior preserved. Storage query/scope/schema implementations unchanged.
- active architecture/guidance/knowledge and new filesystem-boundary-report.md: current owner rules and exact verification. Historical storage-transactions report remains historical.

## Task 1: Generic file contracts and safety primitives

**Consumes:** Existing LocalFileSystem, FileStat (already includes device), CapturedFile, pinned file/directory resources and real confinement/lock/publication algorithms.

**Produces:** Generic FileReader, FileWriter and FileLocker contracts (minimal split; FileWriter may extend FileReader). Core method names: FileReader::file_get_by_path(&self,path:&Path)->FileResult<Option<CapturedFile>>, file_get_stat_by_path, file_is_exist(&self,path:&Path)->FileResult<bool>, directory_get_by_path; FileWriter::file_create_if_absent_by_path(&self,path:&Path,bytes:&[u8])->FileResult<()>, directory_create_by_path, directory_sync_by_path; FileLocker::lock_acquire_by_path/lock_try_acquire_by_path with one actual associated Lock capability. Read-only generic lock-owner observation stays object-safe and uses no application LockName/HeldLock. Add only the generic identity/path helpers required by composition. Exact additional signatures and any real file metadata values are builder choices: record the complete finalized interface in task-1-contracts.md before handing off Task2. No caller is allowed to guess signatures. Constructors/external trait methods retain their conventional names.

- [x] Inventory existing safety tests and capture their baseline names/ignored selectors. Write real generic-file API tests first using neutral file names, including missing/present and I/O errors, no body reads for existence/identity, strict unsafe paths versus stable nonregular entries, parent/leaf replacement, directory pinning, hardlink identity and content updates that keep file identity. Record API-missing compile RED separately from behavioral regression evidence.
- [x] Introduce only necessary file error kinds/results in central contracts, with precise absent/nonregular/unsafe/changed/I/O distinctions and private preserved causes. Keep all newly authored filesystem operations and file-specific errors free of consumer vocabulary.
- [x] Implement generic reads, metadata/existence/identity/path validation, no-clobber writes/sync and generic file locks by retaining the proven low-level mechanisms. Generic parameters/paths select arbitrary files; no config.json/cache.db/vectors.db/index.lock/embed.lock constants in new APIs. Existence/identity never reads file content. Avoid permanently pinning every traversed document directory.
- [x] Keep temporary legacy adapters only to maintain compilability for Task2; do not migrate services or change schemas. Test generic protocols now, including file-lock owner/PID safeguards and existing per-operation race hooks.
- [x] Mutation-check existence/identity, no-follow/alias checks and no-clobber preservation; restore sources and run relevant filesystem tests plus all-target check/Clippy/fmt. Existing legacy tests must remain green. Run a broad root test once if new findings require it; final native acceptance belongs Task3.
- [x] Record finalized interfaces, signatures, error mapping requirements and tests in task-1-contracts.md and task report. Local intended-path commit and independent spec/quality gate before Task2.

## Task 2: Migrate consumers and remove application-specific filesystem APIs

**Consumes:** Exact generic interfaces from task-1-contracts.md and central source declarations; preserve those names/contracts. Any necessary refinement is explicit and recorded, not an undocumented replacement.

**Produces:** All consumers use generic file primitives with caller-selected paths. No DatabasePaths, prepare_paths database-pair API or role-specific filesystem traits/constructors. File-only production filesystem imports/identifiers; generic timing outside filesystem APIs.

- [x] Add/extend caller-level regressions before changing orchestration: absent/invalid config and side-effect-free status, both lock roles and malformed/busy owner metadata, init competing winner/durability failures, database directory/leaf replacement and alias races, document final-entry exclusion versus unsafe-parent failure, unchanged retry/clock behavior. Test new path routing with literal call records or real temporary fixtures, not assertions computed by the code under test.
- [x] Migrate document/embedding/config/status/init/background/source/inspection/provenance services and their tests to generic capabilities, mapping FileError at consuming boundaries. Application role/path constants and status aggregation live outside filesystem; preserve existing public safe messages/codes. Move real retry/time behavior into a minimal injected Clock with a synchronous implementation in process when needed; ordinary per-file capture timestamps can remain file observation data.
- [x] Replace DatabasePaths in composition with generic directory/file observations before/after each existing SQLite open/attach. Preserve safe missing-file handling and raw-fd confinement; normal fixture constructors keep canonical/alias checks using generic LocalFileSystem path APIs. No filesystem collaborator/SQL control moves into another technology implementation.
- [x] Delete role-specific filesystem interfaces, exports, constructors and application-named production helper modules; remove legacy adapters and application-specific error imports from filesystem. Keep a single concrete LocalFileSystem plus genuine private resource/value types. Update AGENTS to agreed boundary/naming rules without losing rafactor's closure/table-first conventions.
- [x] Update active code comments/docs as behavior ownership changes. Existing historical plans/reports remain exact old snapshots with an explicit successor reference where appropriate; no broad unrelated cleanup.
- [x] Mutation-check caller path selection/non-creating status, pre/post verification around open/attach, and retry/lock/init invariants; restore and run affected service/CLI/filesystem/connection suites, all-target Clippy/fmt. Audit physical files/imports/traits/wiring and save reports. Local intended-path commit and independent spec/quality gate.

## Task 3: Native acceptance, current documentation and final review

**Consumes:** Final migrated source and all prior task/mutation evidence. The user subsequently expanded this same branch to Git, Markdown and the other infrastructure components. Keep the Task 1 and Task 2 focused review gates, then run this full acceptance on the final integrated source after those component changes, rather than repeating it on a known intermediate tree. See the [integrated component plan](2026-11-04-infrastructure-component-boundaries.md).

**Produces:** Actual-tree host/Linux PASS receipts, complete invariant matrix, source-linked knowledge checked against exact bytes and an accurate report. Final independent whole-branch review is a separate controller gate after this task.

- [ ] Audit final architecture/naming and active guidance/spec/plan consistency. Update applicable source-linked knowledge with exact Git source-byte verification; preserve old receipt/source basis history. Check affected Markdown links/anchors and executable/design schema bytes unchanged.
- [ ] Run /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host and --platform linux into new .artifacts/filesystem-refactor/host-<timestamp>, linux-<timestamp>. They include make verify; do not duplicate successful full runs. Use isolated CARGO target/evidence, escalation for macOS process tests, pinned Docker Linux runner and fake providers only.
- [ ] On Linux, explicitly run the migrated regression for a UTF-8 symlink whose canonical target has non-UTF-8 bytes, and mutation-check it by temporarily removing the consumer UTF-8 guard. Restore the actual source byte-for-byte before collecting final source-hash receipts; the macOS fixture could not create this path (EILSEQ).
- [ ] Reconcile every receipt input and changed copied source/test file with final bytes; verify all command exits, exact debug/release counts/ignored selectors, native/standalone/linkage and static boundaries. Preserve failed/interrupted attempts separately and diagnose them honestly.
- [ ] Write docs/superpowers/plans/2026-11-04-filesystem-boundary-report.md with all accepted rules, actual API/ownership, invariant results, effective restored mutations and any limits. Commit intended docs/report after self-review; independent task gate, then controller's whole-branch review/fixes.
- [ ] After final review is clean, record closeout and preserve execution evidence directly on the active rafactor branch. Preserve any concurrent user edits. No merge-back from the inactive filesystem-refactor copy, push/PR update or main merge follows without further authorization.

## Preflight review

Task1 deliberately keeps old adapters while producing the exact generic contract; Task2 consumes that contract and removes all old APIs atomically with consumer compilation. Task3 consumes final source/evidence and updates no product behavior. The user approved the boundary and explicitly authorized implementation, and explicitly chose continuing directly on rafactor after the verified transfer. Builder review found no important unresolved design questions; API details remain ordinary recorded builder decisions. No additional approval checkpoint is introduced.
