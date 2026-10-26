# TK-26 Parallel Corruption Preparation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Выполнить независимый этап A TK-26: воспроизводимые corruption fixtures и доказательства текущей безопасной диагностики, не меняя реализацию TK-25.

**Architecture:** Использовать настоящие Git/storage capabilities и продуктовый CLI на собственных временных репозиториях. Продуктовые интерфейсы, runtime, схемы и reindex/detection остаются прежними. Этап B — отдельный план после принятой интеграции TK-25.

**Tech Stack:** Pinned Rust 1.95.0, существующие rusqlite/SQLite/vec1, Git CLI, tempfile, serde_json, текущие test helpers. Новых зависимостей нет.

**Spec:** [Утверждённая спецификация](../specs/2026-10-26-tk-26-corruption-design.md). Пользователь утвердил её сообщением «go on» 2026-10-26. База `913b2b7`, spec commit `3cba476`, ветка `tk-26`; свежий базовый `make check` PASS, `.artifacts/tk26/baseline.json`.

## Global Constraints

- Этот план покрывает только согласованный этап A. Никаких фиктивных reindex tests, заглушек, ignore/TODO вместо этапа B или объявления всей TK-26 завершённой.
- TK-25 в другой сессии владеет reindex/detection/publication и их CLI/composition. Не менять её worktree и не вносить committed production/shared-helper changes. Узкие временные guard mutations — только здесь, с точным restore перед commit/review.
- Все повреждения/удаления — только внутри нового TempRepo/TempDir этого теста, после закрытия собственных соединений/children; не обращаться к настоящей .letopis, Engram, общему object store или чужому процессу.
- SHA-1 и SHA-256 — через FORMATS/фактические OID, без предположения длины 40. Только fixture loose objects; pack/media drills не объявляются выполненными.
- Root-only SnapshotVerifier и closure-checking GitSnapshots::pin — разные контракты. Недостающий child — ошибка целостности pin; отсутствующий root — Missing/not_found; отсутствующий author basis — provenance problem.
- Nonempty corrupt/schema-incompatible БД не становится fresh init. Нулевой SQLite-файл — непубликованная инициализация; история из этих байтов не выводится.
- Corruption после restart означает повторный безопасный command internal_error, не отдельную запись status.problems или историю. files.error — только для реально invalid Markdown при доступной записи.
- Не добавлять auto-delete/repair, recovery.json, error store, runtime, конфигурационные knobs, forwarding bundles или инфраструктуру в domain.
- AAA, явные imports, глагольные имена/get_ accessors, private helpers ниже тестов, функции около40/до60строк. Общий helper появляется только при двух реальных потребителях и общей ответственности.
- Уже корректное поведение может сразу дать GREEN: не фабриковать TDD RED. Различать fixture compile/setup failure, реальную behavioral RED и пойманную временную product mutation; сохранять точные receipts и восстановленные SHA.
- Вывод тестов писать в новые evidence/log files, в чат — результат/счётчики. Host jobs2/tests2/incrementaloff; native/Docker не запускать параллельно кампании TK-25.

## Review Focus

- Corrupt child при доступном root: Task1 проверяет pin, а не ошибочно требует closure traversal от SnapshotVerifier.
- Повреждение fixture не должно уничтожить канон или healthy second DB: Tasks1/2 сверяют независимые bytes/значения и повторное открытие.
- Пустая, отсутствующая и повреждённая непустая SQLite имеют разные ожидаемые исходы: Task2 задаёт их явно.
- Новый CLI-процесс не должен маскировать storage failure успешным revision:null: Task3 проверяет check/status дважды.
- Ошибка Git/SQLite не должна утечь через stdout/stderr/Debug как canary/path/payload: Tasks1/2/3 проверяют безопасную диагностику и single JSON.

## File Structure and Ownership

Create:
- `tests/git_corruption.rs`: повторное наблюдение и восстановление только Git fixture objects, SHA-1/SHA-256.
- `tests/storage_corruption.rs`: физические SQLite corruption controls, отсутствие/пустота/схема, сохранение healthy vectors при bootstrap.
- `tests/corruption_diagnostics.rs`: настоящие check/status в новых процессах, ошибки повреждений и контроль после fixture restore.
- `docs/zkit/letopis/checks/TK-26/report.md`: case→existing/new test→command→result и незавершённый этап B.

Read/reuse, не менять:
- `tests/support/{git,storage,reading,provenance}.rs`, `tests/support/mod.rs`.
- `tests/{git_snapshots,git_retention,snapshot_verification,storage_bootstrap,storage_status}.rs`.
- Продуктовые Git/storage/service/CLI реализации из approved spec.

Modify only documentation metadata at completion of A:
- Этот план; approved spec state; `docs/zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-26-corruption.md`.

Все новые filesystem helpers private в своём test file. Не делать один общий corruption framework для несвязанных Git/SQLite сценариев.

## Task 1: Git corruption across fresh handles and fixture restoration

**Files:** Create `tests/git_corruption.rs`; существующие Git test files — regression coverage, без правок.

**Interfaces:**
- Consumes: `TempRepo::new(ObjectFormat)`, `FORMATS`, `get_root`, `read_oid`, `build_object_path`, `capture_fixture_state`.
- Consumes: `open_git_snapshots`, `SnapshotVerifier::verify_snapshot`, `GitSnapshots::pin`, `SnapshotErrorKind`, `SnapshotState`.
- Produces: private `create_tree_case(repo: &TempRepo) -> (TreeOid, BlobOid)`, `damage_object(repo: &TempRepo, oid: &str, damage: ObjectDamage) -> Vec<u8>`, `restore_object(repo: &TempRepo, oid: &str, original: &[u8])`. ObjectDamage has only Truncate/Missing. Возвращаемый Vec — исходные fixture bytes, не output parameter.

- [x] Сначала составить небольшой coverage index: existing `tree_types_and_missing_objects_are_distinct`, `incomplete_tree_and_ref_failure_do_not_pin`, `verify_snapshot_classifies_absent_foreign_and_non_tree_objects`, `verify_snapshot_checks_root_integrity_without_requiring_child_closure`. Их controls уже задают wrong type/missing root/wrong hash; не копировать одинаковые assertions в новые tests без новой ситуации.
- [x] Написать новые поведения `corrupt_tree_remains_an_error_across_reopen_until_restored`, `corrupt_child_never_pins_an_incomplete_closure`, `missing_child_is_not_a_new_empty_snapshot`. Для каждого FORMATS здоровый контроль pin → закрытие handle → повреждение → две новые операции open/pin → восстановление точных bytes → новый успешный pin. До повреждения проверить независимое содержимое leaf `b"AlphaControl CORRUPTION_CANARY"`, после восстановления — те же bytes.
  ```rust
  assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
  assert_eq!(repo.capture_fixture_state(), original_user_state);
  assert!(!format!("{error} {error:?}").contains("CORRUPTION_CANARY"));
  ```
  При missing child root-only verifier вправе вернуть Available; pin обязан отвергнуть closure. Root отсутствие/not_found остаётся отдельным существующим контролем.
- [x] Запустить `cargo test --locked --offline --test git_corruption`. Зафиксировать честный первый исход. Уже GREEN — characterization; новая behavioral RED — reproducer с конкретной ошибкой. Не начинать production fix вне разрешённого владения.
- [x] Реализовать только private fixture helpers: Git argv создаёт здоровые objects, объектный path берётся из test-owned TempRepo, original bytes снимаются до damage, проверяются regular file/принадлежность fixture. Нет shell interpolation, Git repair, unsafe symlink writes или shared clones. Восстановление происходит в ограниченном test-owned scope.
- [x] GREEN: `cargo test --locked --offline --test git_corruption --test git_snapshots --test git_retention --test snapshot_verification`. Ожидается exit0; текущая default-ignored subprocess entry не считается отдельным ручным запуском.
- [x] Узкая mutation: в owned worktree временно заменить missing-child classification в closure verification с InternalError на NotFound; выбрать covering test выше, получить assertion failure, затем восстановить точные source bytes. Для digest guard использовать уже существующий valid-compressed wrong-object test, если этот guard меняется; не дублировать его.
- [x] Self-review новых файлов/fixture safety, `cargo fmt --all --check`, commit только test file как `test: cover repeated Git corruption rejection`.

## Task 2: SQLite corruption, empty controls and healthy vector retention

**Files:** Create `tests/storage_corruption.rs`; не менять storage implementation/schema/shared helpers.

**Interfaces:**
- Consumes: `open_storage(&Path,&Path)`, Storage/StorageRead/CacheWrite/VectorWrite, Sha256, TempDir и существующий `build_knowledge`.
- Produces: private `seed_vector(cache: &Path, vectors: &Path) -> (Sha256, Sha256)` (config identity/search hash; соединения закрыты при возврате), `damage_database(path: &Path, damage: DatabaseDamage) -> Vec<u8>`. DatabaseDamage has InvalidHeader/Truncated/UnsupportedVersion/PartialSchema. Возвращаемые bytes — фактически подготовленный повреждённый файл для проверки no-overwrite.

- [x] Подготовить собственный TempDir с prefix `CORRUPTION_CANARY` (реальный private path input для anti-leak assertions), здоровый cache с одной известной записью/FTS и revision1, vector identity и точным payload `[0.25_f32,0.75_f32]`, dimensions2. После подготовки закрыть все DB handles. Oracle — literals, не `read_vector` в роли вычислителя ожидаемого вектора.
- [x] Написать `nonempty_corrupt_databases_are_rejected_without_reinitialization`: для каждого из двух DB files и InvalidHeader/Truncated новая fixture, две попытки open дают InternalError, повреждённые bytes остаются прежними; healthy control после восстановления исходных fixture bytes снова читается.
  ```rust
  assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
  assert_eq!(fs::read(&damaged_path).unwrap(), damaged_bytes);
  assert!(!format!("{error} {error:?}").contains("CORRUPTION_CANARY"));
  ```
  Непустое усечение сохраняет некорректные header bytes; не считать корректную маленькую SQLite DB повреждённой только из-за размера.
- [x] Написать `incompatible_existing_schemas_are_never_repaired_implicitly`: version999 и partial schema через PRAGMA/DDL только на закрытой fixture DB. Existing schema tests остаются контролем; новые assertions проверяют повторное открытие и no-overwrite, без копии прежних проверок.
- [x] Написать `missing_and_zero_length_cache_are_unpublished` и `recreated_empty_cache_preserves_healthy_vectors`: отсутствие/0bytes → get_revision()==0; после удаления только cache и его fixture sidecars healthy vector/identity остаются. Это storage bootstrap, не CLI reindex.
  ```rust
  assert_eq!(view.get_revision(), 0);
  assert_eq!(stored_vector, vec![0.25_f32, 0.75_f32]);
  ```
  Никакого требования совпадения всего healthy SQLite-file byte hash: WAL/checkpoint headers могут меняться. Проверять независимые vector bytes/identity и отсутствие повреждений канона.
- [x] Запустить первый focused `cargo test --locked --offline --test storage_corruption`; записать исход, затем добавить minimal private helpers. Использовать TempDir и закрытые owned paths. Не создавать бинарные golden DB и новые product errors/auto-delete.
- [x] GREEN: `cargo test --locked --offline --test storage_corruption --test storage_bootstrap --test storage_status --test storage_vectors`. Для unsafe paths использовать existing bootstrap controls; не подменять permission failure отсутствием.
- [x] Mutation-check unsupported-version fail-closed branch: временно принять неизвестную версию как валидную (без destructive product unlink), covering test должен отвергнуть успешный open; восстановить source hash. Если тест не падает, исправить assertions, не рисовать PASS.
- [x] Self-review/fmt, commit intended test path как `test: distinguish corrupt and unpublished SQLite caches`.

## Task 3: Real CLI restart diagnostics and Stage A handoff

**Files:** Create `tests/corruption_diagnostics.rs`, `docs/zkit/letopis/checks/TK-26/report.md`; update plan/spec approval metadata and TK-26 card only.

**Interfaces:**
- Consumes: `ReadingRepo::new`, `write`, `get_root`, `invoke(&[&str], &[u8]) -> Output`, `open_storage`; `support::provenance::{create_basis,build_note}`.
- Produces: private `assert_internal_failure(output: &Output)` (single JSON/code/exit/safety assertions), private fixture byte snapshots; no production API. Every `ReadingRepo::invoke` is an actual new process with watchdog and isolated Git/credentials. API-only handles from Task1 are not mislabeled process restart.

- [x] Написать `corrupt_cache_and_vectors_fail_in_each_new_command_process`: healthy CLI check сначала публикует известное знание, затем закрытая fixture cache либо vectors заменяется непустыми повреждёнными bytes. По два новых процесса `check`/`status` дают один safe error, не ok/revision:null. После каждого отказа damaged bytes и canonical Markdown/config/user Git state неизменны. Точный original DB snapshot возвращается только для healthy-control проверки.
  ```rust
  assert_eq!(output.status.code(), Some(1));
  let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
  assert_eq!(value["ok"], false);
  assert_eq!(value["error"]["code"], "internal_error");
  assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
  ```
- [x] Написать `corrupt_authored_tree_has_persistent_current_command_failure`: создать здоровый basis через create_basis и note через build_note, healthy check публикует revision1; повреждение root basis даёт hard internal_error в двух новых check/status, а cache revision остаётся1. После restore та же текущая команда успешна. Missing basis — existing soft provenance control, не новый hard failure.
- [x] Запустить `cargo test --locked --offline --test corruption_diagnostics`; зафиксировать первый исход. Fixture-only additions не требуют выдуманной behavioral RED. Не вводить протокол хранения corruption в status.problems.
- [x] GREEN: `cargo test --locked --offline --test corruption_diagnostics --test check_command --test status_command --test provenance_commands`.
- [x] Mutation-check false-fresh-init: временно подавить failed SqliteStatusStorage open в run_status через его существующее Default-непубликованное состояние, проверить падение `corrupt_cache_and_vectors_fail_in_each_new_command_process`, восстановить source bytes. Это только временный guard mutation в этом worktree, не committed production change и не правка reindex/detection.
- [x] Финально `git diff --check`, `cargo fmt --all --check`, `RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 make verify` (включает make check, Clippy, Rustdoc/debug/release). Использовать один bounded CARGO_TARGET_DIR, сохранять полный лог, не повторять suites после неизменённых inputs. Native/Docker не нужен для test-only A: product native/schema/build/dependencies не менялись. Если появился настоящий storage/native fix, это за пределами A и требует отдельного решения/его обязательных runners.
- [x] Отчёт case→test/oracle→command→exit, source/input hashes, fresh process versus handle distinction, mutations+restore hashes, что покрыто existing tests, реальные limits. Card TK-26 становится in_progress; у критериев полного reindex/recovery/MC-2 нет ложных галочек. Этап A не закрывает TK-28 storage-failure criterion, G-1/G-2 или всю TK-26.
- [ ] Независимое review результата A по выбранному workflow, устранить относящиеся замечания и повторить только затронутые проверки. Проверить links/claims, commit test+report+метаданные. Engram — existing shared store, clean-state guard, preserve attributes. Без merge/push/GH closure в рамках этого плана.

## Gate to Stage B — not executable in this plan

После принятой TK-25 отдельный integration plan должен назвать точную dependency
revision и реальные file/API names. До этого никаких skipped stubs или копии reindex.
Он обязан покрыть полный CLI recovery повреждённых/удалённых cache/vectors отдельно
и вместе, reuse здоровых векторов без inference, invalid-current-file/утерянную
last-valid проекцию, диагностику после restart/неуспешной записи и MC-2 interruptions.

Для MC-2 сначала сопоставить старую матрицу TK-2/TK-10 и новые TK-25 parent-controlled
checkpoints с существующими product tests. Релевантный исторический publication
harness запускать отдельно, если нужен; не выдавать его за доказательство нового
product path. Недостающие actual checkpoints/parent assertions добавляются только
после согласования владельца готового TK-25. Полная приёмка и закрытие issue26
зависят от этого этапа, а не от готовности fixture-набора A.

## Self-review and handoff

Scope map: Git matrix→Task1 + existing Git controls; SQLite matrix/zero/missing/healthy
vector→Task2; actual process diagnostics/safe errors→Task3; file/layout/API boundaries
и восстановленные mutations→Tasks1–3. Все пять Review Focus inputs имеют owning
checks. Полный approved spec B намеренно вынесен за границу подготовительного плана;
его обязательства перечислены выше и не объявляются выполненными.

План утверждён 2026-10-26. Пользователь выбрал **Subagent-driven**: последовательные
implementers, task-scoped reviews и независимое итоговое review. TK-25 принята как
`e8eb1a2` и влита в main как `d265333`; этап B в этой ветке не интегрирован и не
реализован. Для него требуется отдельный утверждённый integration plan с точной
полной dependency revision.
