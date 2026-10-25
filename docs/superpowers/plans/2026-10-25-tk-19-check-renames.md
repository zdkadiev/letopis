# TK-19 Production Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Подключить настоящий CLI check к текущей диагностике графа и snapshot, доказать удаление/переименование без изменения корпуса.

**Architecture:** CheckService реализует Checker, выполняет Refresher::refresh и диагностирует один StorageRead через готовый GraphDiagnostics. GitCli предоставляет узкий read-only SnapshotVerifier поверх существующей ObjectDb::validate_tree. lib.rs связывает сервис и существующий background runner; CLI handlers не получают технологических зависимостей.

**Tech Stack:** Rust 1.95.0; synchronous std I/O; существующие Git CLI, SQLite/rusqlite, serde_json, tempfile. Новых зависимостей, runtime и storage schema нет.

**Spec:** [Утверждённая спецификация](../specs/2026-10-25-tk-19-check-renames-design.md).

**Status:** approved пользователем 2026-10-25; выбран Subagent-driven, реализация начата. Ветка tk-19 от main@c080ddb20418327a65b6d49768879920acb03129; baseline make check PASS. Работать только в /Users/zdkadiev/.codex/worktrees/tk-19/letopis.

## Global Constraints

- Читать AGENTS.md и spec вместе с планом; физические модули и реальное wiring обязательны. Concrete struct/impl отдельно от mod.rs; trait application-owned; no Port/Adapter/contracts layers.
- Синхронный Rust, generic dispatch; никакого async runtime, нового process/registry/cache. Git только через argv/stdin внутри git.
- Агент выбирает каталоги и файлы; Git-root и существующий отбор .md/.markdown сохраняются. ID = path/path#anchor; свободные relation names [a-z][a-z0-9_]*, системные includes/external без изменений.
- Check refreshes весь корпус; paths фильтрует findings по точному владельцу. Не внедрять обход каталогов/glob/anchor selectors. Нет автоматического исправления ссылок или created_at.
- Один pinned read после refresh для всех cache-derived findings; граф проверяется до фильтра. Нет inference внутри check/read/write transaction.
- Findings => ok:true/result.errors и exit 0; malformed input/infrastructure/cancellation => ok:false и exit 1. Один JSON stdout. Не менять X-1 envelope.
- GraphDiagnostics TK-18 переиспользуется. Snapshot probe не создаёт tree/blob/ref и не использует pin; ошибка I/O/policy/integrity не маскируется как validation finding.
- status остаётся TK-28, source bytes/ranges/source_changed — TK-21, traversal — TK-23. Пилот/платные запросы/долгие измерения не запускать; G-1/G-2/v1 не объявлять пройденными.
- Методы около 40/max60 строк, caller-before-helper, документированные публичные APIs/#Errors, безопасные static messages/private causes, meaningful constants, тесты Arrange/Act/Assert с независимыми ожиданиями.
- Проверки из root с CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target RUST_TEST_THREADS=2; Cargo --locked --offline. Вывод компактный, полные логи в новом ignored .artifacts/tk19/ каталоге.
- Каждый task заканчивается focused GREEN и выбранным ручным RED mutation control с byte-for-byte restore; никакого полного cargo-mutants sweep. Коммиты только intended paths, без merge/push до указания пользователя.

## Review Focus

1. Wrong-type commit/tag OID нельзя автоматически peel к tree; read-only probe должен отличать недоступное основание от сбоя Git. Task 1.
2. Частично невалидный файл сохраняет прежнюю проекцию; одновременно должны быть видны его текущая ошибка и актуальные graph findings. Task 2.
3. paths на источник не должен делать существующую цель вне paths висячей или скрывать пересекающий границу includes-цикл. Task 2.
4. Отсутствующий путь, одинаковые basename в каталогах, arbitrary relation name и ignored/unmarked файлы не должны нарушать точную фильтрацию или определять новый корпус. Task 2.
5. Конкурирующие вызовы и Git failure после получения graph findings не должны давать потерю публикаций или частичный successful report. Tasks 2–3.

## File Map and Interfaces

- Modify src/service/snapshot_traits.rs: SnapshotState and SnapshotVerifier рядом с существующими snapshot interfaces; src/service/mod.rs: explicit exports.
- Modify src/git/cli.rs: impl SnapshotVerifier for GitCli; повторно использовать ObjectDb::validate_tree из src/git/objects.rs без смены существующих read/pin guarantees.
- Modify src/lib.rs: public wire_checker, раскрытие дополнительного trait у open_git_snapshots, production run_check dispatch и документация доступных команд.
- Create src/service/check.rs: CheckService и его orchestration/helpers; unit module внизу. Если тесты делают файл существенно больше 500 строк, private child src/service/check/tests.rs с #[cfg(test)] mod tests; не дробить production struct/impl.
- Create tests/snapshot_verification.rs: настоящий Git probe; tests/check_command.rs: настоящий binary check; tests/check_lifecycle.rs: rename/delete/concurrency acceptance. Reuse tests/support/reading.rs, documents.rs, git.rs; не менять общие fixtures ради единственного потребителя.
- Modify docs/zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-19-check-renames.md и TK-18-graph-validation.md; create docs/zkit/letopis/checks/TK-19/report.md и verification.json; поддержать spec/plan statuses. Не переписывать общий дизайн ради wiring.

---

### Task 1: Read-only Git snapshot verification

**Files:** service/snapshot_traits.rs, service/mod.rs, git/cli.rs, lib.rs; new tests/snapshot_verification.rs.

**Consumes:** ObjectDb::validate_tree(&TreeOid) -> Result<(), SnapshotError>; существующий self-contained object policy guard.
**Produces:** public enum SnapshotState { Available, Missing, Invalid } с Debug/Clone/Copy/PartialEq/Eq; public trait SnapshotVerifier { fn verify_snapshot(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError>; }. Available = точный tree object с проверенными собственными bytes/hash; Missing = root отсутствует; Invalid = wrong type/foreign OID format. Полную closure не проверяет, children не pin-ит. InternalError остаётся Err.
**Public wiring:** open_git_snapshots(start: &Path) -> Result<impl GitSnapshots + SnapshotVerifier, SnapshotError>; это прежний GitCli с дополнительной capability, отдельный forwarding constructor не нужен.

- [ ] **1. Write RED tests** with real TempRepo in SHA-1 and SHA-256. `verify_snapshot_accepts_exact_tree_without_writes`: use git write-tree fixture then assert `Available`; compare all `for-each-ref` output before/after, HEAD/index/config/live bytes unchanged. `verify_snapshot_classifies_absent_foreign_and_non_tree_objects`: literal absent OID, opposite-format OID, blob, commit and tag => Missing/Invalid; commit/tag не peel. `verify_snapshot_propagates_policy_and_integrity_failures`: added alternates after opening and corrupted loose tree => Err(InternalError), redacted diagnostics; no refs created.

```rust
assert_eq!(git.verify_snapshot(&tree).unwrap(), SnapshotState::Available);
assert_eq!(git.verify_snapshot(&missing).unwrap(), SnapshotState::Missing);
assert_eq!(git.verify_snapshot(&blob_as_tree).unwrap(), SnapshotState::Invalid);
assert_eq!(before_refs, after_refs);
```

- [ ] **2. Run RED:** `cargo test --locked --offline --test snapshot_verification` with shared env. Initially FAIL for missing SnapshotVerifier/SnapshotState export; retain compiler diagnostic, not a claimed behavioral RED.
- [ ] **3. Implement capability:** GitCli checks require_self_contained_objects, calls validate_tree, maps only NotFound to Missing and InvalidRequest to Invalid; InternalError propagates with existing private causes. Document typed states, object integrity versus closure, no side effects. No new Git commands/library required.
- [ ] **4. Run GREEN:** targeted new test plus `cargo test --locked --offline --test git_snapshots --test git_retention`; all PASS. Existing retention assertions unchanged.
- [ ] **5. Mutation:** temporarily return Available for wrong-type outcome; exact wrong-type test must fail. Restore exact source and rerun target PASS.
- [ ] **6. Review and checkpoint:** check architecture/visibility/docs, `git diff --check`; stage only task files; commit `feat: expose read-only Git snapshot verification` when using task commits.

### Task 2: CheckService and actual CLI composition

**Files:** new src/service/check.rs, service/mod.rs, service/command_traits.rs documentation if needed, lib.rs; new tests/check_command.rs. Touch cli input/handlers/output only for a demonstrated integration defect; keep existing transport behavior.

**Consumes:** Task1 SnapshotVerifier; Refresher::refresh(&mut self) -> DocumentResult<RefreshOutcome>; Storage::begin_read; GraphDiagnostics::find_problems<R: StorageRead>(&self, view: &mut R, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>>; Checker::check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>.
**Produces:** crate-private CheckService<D,S,G,V> with new(refresher:D, storage:S, diagnostics:G, snapshots:V) -> Self, impl<D:Refresher,S:Storage,G:GraphDiagnostics,V:SnapshotVerifier> Checker. Public `wire_checker(start: &Path) -> CommandResult<impl Checker + use<>>`. Private `run_check(request: Request, directory: &Path, executable: &Path) -> CommandResult<CommandResponse>` wires checker/background using discovered root like run_search/run_inspect.

**Snapshot findings contract:** validation_failed, details {path, reason, element_id}. Missing external basis: reason `missing_source_snapshot`, message `knowledge source snapshot is missing`. Declared Missing/Invalid basis: reason `source_snapshot_unavailable`, message `knowledge source snapshot is unavailable`, plus details.target = canonical OID. Check all declared OIDs including elements without external edges. One probe per distinct OID per invocation; results attributed to each selected owner. Arbitrary relation types never imply external basis.

- [ ] **1. Write RED service tests**: `check_refreshes_before_opening_one_read`, `check_reuses_same_view_and_probes_each_oid_once`, `check_propagates_refresh_storage_and_git_failures_without_partial_report`. Use instrumented collaborators and real temporary storage where practical; do not implement dozens of unrelated fake storage methods if existing fixture suffices. For pinned-view scenario, injected diagnostics commits revision+1 through a separate connection after observing old view; snapshot metadata must still come from old view. Git error after graph findings must return Err, never a CheckReport.
- [ ] **2. Write RED binary tests** using ReadingRepo and CARGO_BIN_EXE_letopis, not TK11 fixture executable. `check_binary_reports_multiple_errors_and_repairs`: two malformed files + valid source to absent target + separate includes cycle, assert exact safe ordered errors and valid-file publication. Repeat after repairing each cause => empty errors. Literal errors include `dangling_target`, `duplicate_anchor`, `nested_element`, `includes_cycle` as appropriate; use exact TK18 fixture messages/details, do not derive expected JSON with production formatter.

```rust
let output = repo.invoke(&["check"], b"{}");
assert_eq!(output.status.code(), Some(0));
let response: Value = serde_json::from_slice(&output.stdout).unwrap();
assert_eq!(response["ok"], true);
// In a dedicated one-dangling fixture, assert the literal output, not only len().
assert_eq!(response["result"]["errors"][0]["details"]["target"], "missing.md");
```

- [ ] **3. Add boundary cases:** `check_paths_filters_owners_after_full_graph_validation`: source inside selection, valid target outside, cross-file cycle, same basename `a/item.md`/`b/item.md`, duplicate/normalized selectors, absent selector => exact expected subset and no false dangling. `check_preserves_agent_layout_and_relation_names`: nested .markdown in knowledge/, ordinary unmarked .md, ignored new file, arbitrary valid `explains_case7` edge; preserved bytes and no enum/list restriction. `check_snapshot_findings_are_current_and_safe`: missing metadata, valid/absent/blob/foreign OID, supplied OID without external edges, repeated OID and repair; exact reason and owner. `check_binary_rejects_invalid_input_and_infrastructure_failure`: malformed JSON/path => invalid_request exit1 before .letopis init; symlink service path => safe error, unchanged external sentinel. serde_json::from_slice of whole stdout rejects a second JSON; stderr empty on normal findings.
- [ ] **4. Run RED:** `cargo test --locked --offline --test check_command`; current binary must fail success assertions with operation unavailable. Service target `cargo test --locked --offline --lib service::check` initially fails compile/not-found until tests connected; distinguish compile RED from behavioral RED.
- [ ] **5. Implement CheckService:** refresh, begin_read once, diagnostics on that view, then nodes/edges on same view to derive external owners and snapshot findings. Cache distinct OID outcomes in invocation-local BTreeMap (no durable cache); merge/sort/dedup by path/reason/element_id/target using a local helper. Preserve graph/file findings and projected metadata of invalid files. Filter owner paths before Git probes so unrelated I/O cannot fail scoped check. Map refresh/storage/probe InternalError to safe CommandError with retained cause, without partial response.
- [ ] **6. Wire actual CLI:** construct refresher and independent confined storage via existing setup pattern, CurrentGraphDiagnostics::new and GitCli; dispatch Request::Check through Checker and existing BackgroundRunner. Missing model config works with no inference; do not preflight/call HTTP for foreground validation. Update stale run_cli documentation that check is unavailable.
- [ ] **7. Run GREEN:** `cargo test --locked --offline --lib service::check` and `cargo test --locked --offline --test check_command --test graph_diagnostics --test cli_runtime`; then `make check`. Tests must PASS without external endpoints. Existing background unit/CLI tests establish launch sequencing; no new real provider call.
- [ ] **8. Mutations:** bypass refresh => refresh/current-state test RED; filter graph before validation => cross-boundary case RED; treat Git infrastructure error as clean => error-propagation test RED; remove OID dedup => counting test RED. Each mutation restored independently, final focused GREEN.
- [ ] **9. Review and checkpoint:** verify dependency direction, one read, handler/service split and actual constructors in lib.rs; commit only intended task files as `feat: wire production corpus check through current diagnostics` when using task commits.

### Task 3: Rename/delete/concurrent acceptance and final report

**Files:** new tests/check_lifecycle.rs; TK19 card, TK18 card, spec/plan; new checks/TK-19/report.md and verification.json. Reuse existing helpers; any product defect discovered here returns to its owning Task1/2 or existing refresher with narrowly scoped regression first.

**Consumes:** public wire_checker and actual binary from Task2; DocumentRepo/ReadingRepo, existing Refresher/StorageRead to independently observe metadata, owned lock/resource handles for controlled overlap.
**Produces:** real CLI lifecycle evidence, documented completed-versus-pending acceptance, no new product abstraction.

- [ ] **1. Write lifecycle tests:** `check_tracks_file_and_directory_rename_without_touching_sources`: tracked target `knowledge/old.md`, unchanged source pointing to it; rename => literal dangling, new path node retains created_at=42, old node absent; restore target => no problem; rename nested directory similarly. Assert source bytes remain identical after every step. `check_tracks_anchor_rename_removal_and_repair`: target #old -> #new => dangling #old, metadata created_at=42 unchanged; restoring anchor or repairing source resolves it. `check_keeps_last_valid_projection_while_reporting_current_file_error`: invalid edit retains old node and emits current parser error; repair updates metadata/results. Use separate focused tests/helpers so no function exceeds60 lines.
- [ ] **2. Add `concurrent_check_publishes_edits_from_both_writers`:** initialize repo, construct separate services, two scoped threads each edit a distinct file then wait on one Barrier before check; both results succeed, final pinned cache has both literal texts/IDs and original literal created_at values. No arbitrary sleeps or global cwd/env mutation; no requirement for exact intermediate revision count. Concurrent returned reports may observe different committed revisions; final state must contain both edits.
- [ ] **3. Add `repeated_check_preserves_corpus_and_revision`:** record bytes of marked/unmarked files and cache revision after first check; repeated unchanged check returns identical findings with unchanged bytes/revision, including missing targets. Reuse actual binary for lifecycle acceptance, and service concurrency where that gives deterministic scheduling without production hooks.
- [ ] **4. Run targeted tests:** `cargo test --locked --offline --test check_lifecycle`; new tests are acceptance extensions over Task2, so may start GREEN. Never invent a RED: prove selected assertions by mutation. If a regression is found, retain its failing output before the narrow fix.
- [ ] **5. Mutation controls:** suppress removal publication => rename/delete case RED; replace authored timestamp in projection with another value => literal42 check RED; temporarily drop one changed file from the detected publication delta => final two-file assertion RED. Эти controls проверяют наблюдаемый результат, не требуют новой стратегии блокировок и не оставляют production hooks. Restore files byte-for-byte and rerun all three new targets GREEN.
- [ ] **6. Run final validation once:** `make verify` (includes check, strict Rustdoc, release tests), `git diff --check`; retain logs and hashes of actual changed sources. No native stack/schema/dependency edits anticipated, so historical storage harness is not rerun by default. If those change, run relevant host/Linux runner with new evidence and report platform limits honestly.
- [ ] **7. Review actual tree:** physical modules/visibility/trait impls/import direction/lib wiring, sync-only execution, no body/schema/Markdown rewrite, stdout/error safety, RAII releases and tests independent of external model. Native execution ends with one whole-branch reviewer; SDD adds its required task gates.
- [ ] **8. Write report/receipt:** base/source tree hashes, macOS host/toolchain, exact executed commands/exits, meaningful test cases, mutation outcomes and restored hashes, architectural review, link checks, unrun Linux/harness/pilot limits. New private logs under .artifacts/tk19/; don't overwrite historical TK18/TK17 evidence. Report source revision accurately even if uncommitted.
- [ ] **9. Update cards:** TK19 marks proven check/rename/created_at criteria separately; status consumer stays pending TK28 and cannot be claimed from component tests. TK18 references real check evidence but keeps status/traverse pending. Spec approved, plan execution status/current results updated. GitHub update/closure and merge/push require user instruction; no premature done/closure when combined status criteria remain unmet.
- [ ] **10. Record Engram and checkpoint:** shared ENGRAM_DB=/Users/zdkadiev/Repos/letopis; fresh status/tokens, preserve all existing attrs/provenance, record focused findings/verification with intended source revision. Commit intended tests/report/docs as `test: prove check rename and concurrent publication lifecycle` when using task commits.

## Self-review and Handoff

- Spec items1–3,6–9 -> Task2; source_snapshot4–5 -> Tasks1–2; authored time/no file writes10 -> Task3. Caller-owned view and no partial success tested in Task2; cancellation retains existing runtime with no new signal implementation.
- All five Review Focus entries have named owning tests above; type and constructor names match each Interfaces block.
- Plan uses existing interfaces/fixtures, no new dependency and no copying domain graph validation. Documentation-only plan preparation does not rerun unchanged product suites; baseline PASS belongs to original base.
- User approved the plan and chose Subagent-driven on2026-10-25. Execute tasks sequentially with fresh task implementer/reviewer, then whole-branch review. Native was the original recommendation, not the selected method.
