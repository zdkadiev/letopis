# TK-26 Corruption Recovery Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (already selected by the user) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Закрыть оставшиеся критерии TK-26: настоящий CLI reindex после явного удаления повреждённых производных БД, сохранение здоровых векторов и свежая регрессия MC-2.

**Architecture:** Потребить единственный `DocumentRefresher` и CLI reindex из принятой TK-25. Новые integration tests управляют только собственными fixtures; продукт не получает repair-команд, автоматического unlink или второго recovery algorithm. Существующие process schedules доказывают атомарную публикацию; новый framework не создаётся.

**Tech Stack:** Pinned Rust 1.95.0, существующие Git/SQLite/vec1, tempfile, serde_json и test helpers. Без новых зависимостей и платных/реальных модельных вызовов.

**Spec:** [Утверждённая спецификация TK-26](../specs/2026-10-26-tk-26-corruption-design.md), разделы «Этап B», «Регрессия публикации MC-2».

**Dependency checkpoint:** TK-25 принята в `e8eb1a2`, merge main `d26533311e1443da01f7368625c9557353bd76f2`. Этап A остаётся исторически проверенным на своей базе `913b2b7`; его evidence не переносится на новую ревизию без запуска. План **утверждён** пользователем сообщением «go» 2026-10-26; начато Subagent-driven выполнение B. Ветка остаётся `tk-26`.

## Global Constraints

- Markdown/Git — канон; SQLite — пересоздаваемая проекция. Не удалять всю `.letopis`, config, lock files, пользовательские refs или авторские bases.
- Удаление выполняет только тест/operator после закрытия собственных DB handles/children: точный owned database path и его `-wal`/`-shm`, без glob, directory cleanup или обхода symlink. Повреждения только в новых test-owned fixtures.
- Nonempty corrupt/schema-incompatible БД не становится fresh init. Corruption сначала даёт безопасный `internal_error`; только явное удаление разрешает initial build. Нулевой файл не доказывает утраченную историю.
- Здоровый vectors.db сохраняет identity и точные векторы по прежнему search_hash; не требовать byte-identical всего SQLite файла из-за WAL/checkpoint. Потерянный vector payload нельзя объявлять восстановленным без обычного inference.
- После удаления cache publication counter начинается заново: первая новая публикация — revision 1. Если удалён только vectors.db, cache revision сохраняется и следующий reindex увеличивает её на 1.
- Status наблюдает и не создаёт отсутствующий cache. Каждый CLI invocation — новый процесс. При доступной записи invalid Markdown даёт files.error; инфраструктурная ошибка не выдаётся за сохранённую строку.
- После потери last-valid cache не восстанавливать старый invalid файл из несуществующей истории. Валидный B публикуется, текущий invalid A диагностируется; канонические bytes сохраняются.
- Помимо интеграции неизменённой принятой TK-25, никаких новых production/shared-helper/schema/native/dependency изменений в исходном объёме B. Найденный продуктовый дефект — точный reproducer и решение по владельцу, без ослабления assertions/ignore и без самовольной смены дизайна.
- KISS/DRY/YAGNI, существующие hexagonal boundaries; domain не получает I/O. AAA, независимые literals, caller-before-callee, явные imports, глагольные имена, около 40/максимум 60 строк, не более двух уровней вложенности.
- Уже работающее поведение может сразу быть GREEN. Setup/compile failure не behavioral RED. Узкие временные mutations разрешены только с точным restore и повторным GREEN до commit.
- Запуски последовательные, jobs2/tests2/incrementaloff. Полные логи в новых evidence paths, краткий итог в чат. Не повторять прошедший тяжёлый suite без изменённых входов или конкретного сомнения.
- Не сливать tk-26 в main, не push/close GitHub issue в рамках этого implementation plan. G-1/G-2, пилоты, физическая потеря питания, media/pack damage и TK-38/39 не объявляются пройденными.

## Review Focus

- Потеря cache при целых vectors: восстановить граф/FTS/files и прежние vectors, не вернуть старый counter и не обращаться к модели — Task 1.
- Потеря vectors при целой cache: не потерять текст/Markdown и не сообщить выдуманное embedding coverage — Task 1.
- Удаление обеих БД и повторный запуск: корректный initial build без скрытого auto-repair, status до reindex не создаёт cache — Task 1.
- Invalid A после потери last-valid projection: видимая текущая ошибка, valid B опубликован, старое знание A не выдумано — Task 2.
- Corrupt required Git object и прерывание до/после commit: safe failure/old-or-complete-new view, освобождение lock и повторное обнаружение — Task 2.

## Files and Ownership

Create:
- `tests/corruption_recovery.rs`: реальный CLI, missing/corrupt cache/vectors, независимые graph/FTS/files/vector oracles; private owned DB helpers.
- `tests/corruption_reindex.rs`: reindex при Git corruption и invalid/valid corpus после потери кэша; private scenario helpers.

Read/reuse without modification:
- `tests/support/{reading,git,storage,provenance,embedding}.rs`.
- `tests/{reindex_command,document_reindex,git_corruption,storage_corruption,corruption_diagnostics}.rs`.
- `src/service/document_refresher/tests/{process,reindex_process}.rs`: acknowledged child checkpoints/MC-2 regression.
- `src/service/document_traits.rs`, existing CLI/composition, storage implementation.

Documentation updates at completion: this plan, TK-26 spec/card/report. Historical Stage A evidence remains labelled by its captured source. New evidence goes under a new `.artifacts/tk26/stage-b-*` directory.

## Task 1: Integrate accepted TK-25 and prove physical cache recovery

**Files:** Create `tests/corruption_recovery.rs`; merge accepted main into the current feature branch, never switch or mutate the other session's worktree.

**Interfaces:**
- Consumes `Refresher::reindex(&mut self) -> DocumentResult<RefreshOutcome>`; actual binary `ReadingRepo::invoke(&["reindex"], b"{}") -> Output`, successful result `{ "revision": N }` inside existing X-1 envelope.
- Consumes `ReadingRepo::{new,write_knowledge,write_config,seed_vectors,open_storage,get_root}`, `Storage::begin_read`, existing read methods and `VectorOutcome::Current`.
- Produces only private test helpers: `invoke_reindex(repo: &ReadingRepo) -> Value`, `remove_owned_database(root: &Path, name: &str)`, independent assertions. Keep helpers local; do not expose an abstraction for a future task.

- [x] Verify clean `tk-26`, record HEAD; merge exactly `d26533311e1443da01f7368625c9557353bd76f2` into this branch. If local main has advanced, inspect the additional range first rather than silently widening the dependency. Read updated AGENTS/design; preserve existing user work. Record merge/source revisions.
- [x] On merged source, run `cargo test --locked --offline --test git_corruption --test storage_corruption --test corruption_diagnostics --test reindex_command --test document_reindex`; preserve first outcome. Resolve a genuine integration regression before adding recovery claims; do not attribute historical Stage A full verify to merged code.
- [x] Write `reindex_recovers_explicitly_removed_cache_vectors_or_both`: independent cases for targets cache/vectors/both, each initially Missing or NonemptyCorrupt. Publish `a.md`=`AlphaToken [peer](b.md "letopis:type=context")`, `b.md`=`BravoToken` to revision 3 before damage. With no config, inference is unavailable. Close all own handles before damage. NonemptyCorrupt first fails reindex and status twice with single safe JSON/exit1, unchanged corrupt bytes and canonical state; then explicitly remove only selected owned DB files/sidecars. Missing uses the same removal without inventing an error. Status after cache loss returns revision:null and leaves cache absent. Reindex then publishes actual nodes/files/FTS from both Markdown files. Expected revision: 1 if cache removed, otherwise 4. Exact knowledge IDs/tokens, edge `a.md`→`b.md` type `context`, and valid files, not only node counts. Missing vectors imply no stored vectors/identity, not recovered embeddings. Repeat reindex/new process to prove stable recovery and next revision.
- [x] Write `reindex_preserves_healthy_vectors_after_corrupt_cache_removal`: actual healthy initial publication; valid local fixture config; seed **all** current knowledge hashes, a=`[0.25_f32,0.75_f32]`, b=`[0.75_f32,0.25_f32]`, dimensions 2. Capture identity, hashes, config/canonical bytes. Corrupt cache first fails; explicit cache-only removal and reindex restore both nodes/FTS/files and exact literal vectors with same hashes/identity. Keep a test-owned embed lock across reindex to exclude drain/waits. A local `support::embedding::HttpFixture` records zero provider requests; no live endpoint/credentials. Check after dropping/reopening storage. Do not compare whole vectors.db file hashes.
- [x] Implement minimal private helpers; all deletions require canonical parent owned by fixture and closed resources. Include default staged/unstaged `live.md`, explicit notes/source/config, HEAD/index/user refs in preservation oracle. Healthy peer DB uses independent semantic values; sentinel outside the selected files stays intact. No fixed sleeps or unowned background children.
- [x] Run `cargo test --locked --offline --test corruption_recovery --test reindex_command --test document_reindex`, fmt and focused Clippy. Record real exit/counts. Mutation-check vector retention by temporarily returning `Ok(VectorOutcome::Current(None))` instead of the final lookup in `StorageRead::read_vector` in `src/storage/read.rs`; covering literal assertion must fail at runtime, then restore exact source hash and rerun covering test. Do not build a second reindex or mutate shared stores.
- [x] Commit intended test path and integration checkpoint; report actual commands, environment, source hashes, mutation restore and concerns for task review.

## Task 2: Git/invalid-file recovery, MC-2 regression and final acceptance

**Files:** Create `tests/corruption_reindex.rs`; update TK-26 report/card/spec and this plan. No edits to existing process harness/product code.

**Interfaces:**
- Consumes same actual `ReadingRepo::invoke`/X-1 command response and existing `create_basis`, `build_note`, owned loose object helpers/patterns from Stage A; no cross-test import or helper framework.
- Consumes accepted process tests under `service::document_refresher::tests::process` including child module `reindex_process`; these already invoke real adapters and acknowledged checkpoints.
- Produces private `assert_safe_failure(output: &Output)` plus fixture-owned helpers only; report maps each MC-2 condition to its actual test and source.

- [x] Write `reindex_rejects_corrupt_required_basis_without_advancing`: healthy note references a real source_snapshot, publication revision1; close handles and corrupt a **required** root tree or referenced child blob (separate fixture per case). Two fresh reindex processes fail safely, not fresh init. Read-only oracle retains revision1 and complete old graph/FTS/files; no corrupted payload/private path in output, no new retained replacement basis or canonical changes. Restore original object bytes only as a fixture control, then actual reindex succeeds at revision2. Stage A remains the SHA-1/SHA-256 and missing-root/type distinction coverage; do not duplicate its whole API matrix.
- [x] Write `reindex_after_cache_loss_reports_invalid_file_and_publishes_valid_peer`: initial valid A=`AlphaToken`, B=`BravoToken`, revision1. Replace A with malformed `<!-- letopis: CORRUPTION_CANARY -->`, B with `CharlieToken`; retain canonical bytes. Delete only closed cache and sidecars. Actual reindex returns revision1 (fresh counter). FTS has Charlie/B, no Alpha/A or Bravo; files.error for A/status invalid-file problem appears without raw payload. Repeat new status to prove current diagnostic. Fix A to `DeltaToken`, reindex revision2; Delta/A and Charlie/B present, error gone. Both stages preserve current Markdown, default live.md, config and user Git state. Never claim old invalid A recovered from lost history.
- [x] Run new tests first and record honest characterization GREEN/behavioral RED; implement private fixture/oracle helpers only. Run `cargo test --locked --offline --test corruption_reindex --test corruption_diagnostics --test provenance_commands --test reindex_command`. Mutation-check preservation of invalid diagnostic by temporarily returning `error: None` in the `FileRow` construction after validation in `decode_file` in `src/storage/files.rs`, demonstrate covering assertion failure, restore exact bytes and rerun covering test.
- [x] Fresh MC-2 regression: `cargo test --locked --offline --lib service::document_refresher::tests::process -- --nocapture` plus `cargo test --locked --offline --test document_refresh --test document_reindex --test reindex_command`. Confirm matched names/counts; zero selected tests is not PASS evidence. Map existing `reindex_sigint_preserves_exact_old_or_complete_new_state` cuts locked/begun/fts/before_commit/after_commit, WAL-pinned reader, contending publisher/redetection, late SQL rollback/released lock; existing normal-refresh process schedules cover other mutation phases. Child ignored entries are executed by parent schedules, not hand-waved. Reuse controls; no sleeps/new checkpoint API. Signal death is not TK-29 handled cancelled JSON or physical power-loss proof.
- [x] Final `git diff --check`, `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 make verify V=1` with one bounded target directory and new full log. It includes check/fmt/Clippy/Rustdoc/debug/release; no duplicate broad run. Test-only B does not require another native/storage campaign. If product/storage/build changes become necessary, stop claiming this scope suffices and apply required host/Linux runners to those inputs.
- [x] Update report with case→test→independent oracle→command/exit and actual source/input hashes; distinguish original A, merged-source baseline and final B. Mark only evidenced criteria done; record Linux not run here, no paid inference, no media/powerloss/gate claims. Update TK-28 storage handoff by a reference in TK-26 report, not by claiming all TK-28 acceptance complete. Run local Markdown link/consistency checks.
- [x] Commit intended tests/docs; task-scoped review then independent final whole-branch review. Record remaining findings/limits in report and Engram; retain branch for user-directed merge/issue update/push.

## Self-review and approval gate

Coverage: cache/vectors/both, missing/corrupt and vector reuse→Task1; Git required objects, invalid A/valid B and restart→Task2; MC-2→existing accepted schedules rerun by Task2; preservation/safe errors→both tasks. Native schema/bootstrap distinctions stay in Stage A. No new product API, storage model or orchestration layer.

The user already selected Subagent-driven. The user approved this Stage B plan with “go” on 2026-10-26. Execute with the preserved Subagent-driven method.

## Execution checkpoint — 2026-10-26

Task 1 integrated exact accepted main at `0f980e1`, implemented recovery tests in
`1da09f8`, and passed controller task reviews. Its `make check` outer count is
774 passed/0 failed/5 ignored; 8 nested child summaries are excluded from that count.
Task 2 tests, diagnostic mutation/exact restore and fresh MC-2 schedules are recorded
in the [Stage B report](../../zkit/letopis/checks/TK-26/report.md#stage-b).
Final `make verify V=1` exited 0 with terminal ok: debug/release each 776 outer
passed/0 failed/5 ignored across 59 executable targets; 8 nested child summaries
per profile excluded. Final source/input and full-log hashes are recorded there.
Task-scoped reviews and final independent whole-branch review `d265333..70a4fdf`
приняты без замечаний. Engram получает окончательный результат после metadata commit.
Ветка сохранена; main merge/push/GitHub close не выполняются в этом плане.
