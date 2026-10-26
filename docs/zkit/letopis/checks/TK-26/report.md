# TK-26 — evidence независимого этапа A

Дата: 2026-10-26. Состояние карточки: **in_progress**. Утверждённая
[спецификация](../../../../../docs/superpowers/specs/2026-10-26-tk-26-corruption-design.md)
разделяет независимый этап A и интеграционный этап B. Этот отчёт подтверждает
только A; итоговый независимый review и scoped review исправления приняты. TK-25 принята как
`e8eb1a2` и влита в main как `d265333`, но этот Stage A branch остаётся на своей
проверенной базе и не потребляет новую реализацию.

## Результат и граница

Добавлены три изолированных integration-набора без production/shared-helper правок:

- `tests/git_corruption.rs` — повторное fail-closed чтение loose Git objects через
  свежие handles для SHA-1/SHA-256 и точное fixture-восстановление;
- `tests/storage_corruption.rs` — повреждённые/несовместимые SQLite, отдельные
  missing/zero controls и сохранение здоровых vectors при пустом cache;
- `tests/corruption_diagnostics.rs` — настоящие новые процессы `check`/`status`
  после повреждения cache, vectors и авторского source basis.

Коммиты этапа: Task 1 `5d49337599fa82306e9e454c0ae402a0d308fa28`,
Task 2 `9af635f20928e0aac6e4dddb015ccbcbc61d5ab7`, Task 3
`17a8c824d86c5d2b0c34ccb0016391f59f2566d4`, review fix `ee44952`.
Product Rust, contracts,
schema, native stack, dependencies и TK-25 reindex/detection не менялись.

## Case → oracle → evidence

| Случай | Test / независимый oracle | Фактический результат |
| --- | --- | --- |
| Повреждённый root tree | `corrupt_tree_remains_an_error_across_reopen_until_restored`; literal leaf payload и captured user state | Два fresh handles на SHA-1/SHA-256 дают `InternalError`; exact loose bytes restore read/pin |
| Повреждённый/отсутствующий child | `corrupt_child_never_pins_an_incomplete_closure`, `missing_child_is_not_a_new_empty_snapshot` | Root-only verify остаётся отдельным contract; closure pin дважды fail-closed, не fresh init |
| Непустой повреждённый cache/vectors | `nonempty_corrupt_databases_are_rejected_without_reinitialization`; revision 1, literal FTS row/vector `[0.25, 0.75]` | Invalid header/truncation дважды дают `InternalError`, damaged bytes не переписаны |
| Несовместимая SQLite schema | `incompatible_existing_schemas_are_never_repaired_implicitly` | version 999 и partial schema отвергнуты повторно без implicit repair |
| Missing/zero SQLite | `missing_and_zero_length_cache_are_unpublished`, `missing_and_zero_length_vectors_preserve_published_cache` | Cache становится только unpublished revision 0; missing/zero vectors не стирают published cache revision 1 |
| Пустой cache + healthy vectors | `recreated_empty_cache_preserves_healthy_vectors` | Storage bootstrap сохраняет identity и literal vector; это не CLI reindex/reuse claim |
| Corrupt cache/vectors в новых CLI | `corrupt_cache_and_vectors_fail_in_each_new_command_process` | Для каждого DB два `check` и два `status`: exit 1, один JSON, `internal_error`, никогда ok/revision:null |
| Corrupt authored root basis | `corrupt_authored_tree_has_persistent_current_command_failure` | Два `check` и два `status` повторяют hard failure; cache DB остаётся exact revision 1; exact object restore возвращает обе команды к success |
| Missing author basis | Existing `missing_basis_is_diagnosed_without_dropping_knowledge` | Остаётся soft `validation_failed` provenance diagnostic; не переопределён как corruption |

Каждый `ReadingRepo::invoke` запускает фактический `CARGO_BIN_EXE_letopis` как
новый supervised process с изолированными Git settings/credentials и 30-секундным
watchdog. Прямые `open_storage` и Git handles используются только как API-oracles
revision/closure и не называются process restart.

Canary находится в реальных fixture inputs: leaf/source/knowledge payload и
повреждённые DB bytes. Error stdout/stderr не содержит `CORRUPTION_CANARY` или
уникальный private-root marker `letopis-git-`; stderr пуст, stdout содержит ровно
одну строку JSON. После каждого отказа повреждённые bytes, canonical Markdown,
отсутствующий config, Git HEAD/index/config/user refs и здоровая вторая DB остаются
неизменны. Точные исходные DB/object bytes возвращаются только перед healthy control.

## TDD, focused runs и mutation receipts

- Task 3 first executable run:
  `cargo test --locked --offline --test corruption_diagnostics` — exit 0,
  2 passed. Это честный characterization GREEN существующего поведения, не
  выдуманный behavioral RED.
- Focused regression:
  `cargo test --locked --offline --test corruption_diagnostics --test check_command --test status_command --test provenance_commands`
  — exit 0, 22 passed (2 + 7 + 7 + 6), 0 failed.
- Task 1 focused matrix — exit 0, 26 passed; classification mutation — exit 101,
  intended assertion failure. `src/git/refs.rs` restored from mutated
  `0cb206c7…63e4d0` to `af6367bc…507bbd`.
- Task 2 focused matrix — exit 0, 38 passed; unsupported-version mutation —
  exit 101, intended acceptance failure. `src/storage/schema.rs` restored from
  `43c77f60…46ce8` to `a09e302f…5675f8`.
- Task 3 false-fresh mutation changed only `run_status` temporarily so failed
  `SqliteStatusStorage::open_confined` yielded its default unpublished state.
  The exact DB test failed exit 101 on the false exit 0 / `revision:null` response.
  `src/lib.rs` mutated hash `de830f1d…1d0784`, restored hash
  `f97ebb7a…37ce9a0`; `git diff --exit-code -- src/lib.rs` returned 0 and the
  restored exact test passed.

Full raw logs are retained only in the ignored plan workspace
`.superpowers/sdd/2026-10-26-tk-26-corruption-preparation/`; historical Task 1/2
receipts were not rewritten.

### Review fix round 1

Scoped review found that canonical snapshots named the scenario-specific Markdown
and config but omitted the staged/unstaged `live.md` fixture created by
`ReadingRepo::new`. A product regression could therefore overwrite that canonical
file without failing either new test. Commit `ee44952` adds `live.md` to every
`UserFixtureState` capture and reorders the affected private helpers so each caller
precedes its callees, as required by AGENTS.md. Expected state was not weakened.

The test-only oracle control temporarily wrote `test-oracle negative control` to
the owned fixture `live.md` after a real failed command and before the state
assertion. The exact database corruption test exited 101 at runtime on the
`UserFixtureState` mismatch. This was an oracle control, not a product fix or
product mutant; the temporary line was removed and the test source restored to
SHA-256 `f431ae3dd6cde7951d0253d9f69e4bda10b336f89d859075d3957b33c4f9b235`.

After restoration, the required CLI/provenance matrix passed 22/22, rustfmt and
focused Clippy passed, and one new final `make verify V=1` completed with terminal
`ok`: debug and release each 750 passed / 0 failed / 5 ignored across 55 executable
targets. Its 2,295-line raw log has SHA-256
`86fa206d594a2a7acc796e8abbab4d37aa21673209ac34beee17133b457908ab`.
The earlier `17a8c82` verification remains historical evidence; it is not presented
as covering the strengthened canonical-state oracle.

### Final whole-branch review fix wave

The final review found one Minor readability issue: the cache/vectors case nested
target, command and repetition loops three levels deep. The inner two loops were
replaced by one literal sequence `["check", "check", "status", "status"]` inside
the target loop. Command counts, process freshness and all byte/state/error oracles
are unchanged; no assertion or expected state was weakened.

The exact focused command
`cargo test --locked --offline --test corruption_diagnostics` exited 0 with 2/2
passed. `cargo fmt --all --check` and targeted
`cargo clippy --locked --offline --test corruption_diagnostics -- -D warnings`
also exited 0. Current test SHA-256 is
`13fd42e8bc71e7c29311ef987fb742f09a5b3a4474e71b84d95b35004dc9d73d`;
the 15-line focused log SHA-256 is
`00000f96470290e448a5eedfff4eca6344af2945f8f822f5d7f8e88bde3a75ce`.
The preceding full verify remains evidence for source hash `f431ae3d…4f9b235`.
It was not rerun after this mechanical same-behavior amendment, by explicit review
direction; the latest covering evidence is the focused target plus fmt/Clippy.

## Финальная host-проверка

Build controls: `CARGO_TARGET_DIR=/private/tmp/letopis-tk26-task1-target`,
`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, `CARGO_INCREMENTAL=0`.

- `git diff --check` — exit 0.
- `cargo fmt --all --check` — exit 0.
- `make verify V=1` — exit 0, terminal `ok`: fmt/check, Clippy and strict Rustdoc
  passed; debug **750 passed / 0 failed / 5 ignored** in 55 executable targets;
  release **750 passed / 0 failed / 5 ignored** in the same 55 targets. The new
  diagnostics target passed 2/2 in both profiles.

Counts use the last outer libtest summary for every Cargo `Running` target, so
nested supervised child summaries are not counted twice. The 0-test Doc-tests
target is excluded. Current post-review raw log: 2,295 lines, SHA-256
`86fa206d594a2a7acc796e8abbab4d37aa21673209ac34beee17133b457908ab`.

Native/Docker storage runner не запускался: production storage/schema/native/build,
vendored inputs и зависимости не менялись. Отдельный `make check` не запускается,
поскольку входит в финальный `make verify`.

## Source/input hashes

| Input | SHA-256 / revision |
| --- | --- |
| Stage A plan base | `1d11051c6d3f7208912b5d95930e36dad85722ce` |
| Task 3 committed tree | `2cfd542d19a53f0fe0a8a4936468afa7754fea64` |
| `Cargo.toml` | `3fe6c97ed98cf57d1bb20856491914c60ee951ce4f6ae99261f5b9c3d0353bff` |
| `Cargo.lock` | `0065802223d81cdbd244e308e7014eba2c132a42ebaa7fb844ca10524a9a93ee` |
| `rust-toolchain.toml` | `12dc5d64dbd96c1b07be2943241d52d9f6127a79ba47a820455ba7b873fc896a` |
| `tests/git_corruption.rs` | `0b46907e218ce7dd98a982a191d455aa7b8cc9541ffc412b2e03561a847722ea` |
| `tests/storage_corruption.rs` | `01c03c3a93ab32a955689da072f3c31b2e8173eeacc82685391d9fdac55e0968` |
| `tests/corruption_diagnostics.rs` | `13fd42e8bc71e7c29311ef987fb742f09a5b3a4474e71b84d95b35004dc9d73d` |

Host: macOS 26.6.2 / Darwin 25.6.0 arm64; `rustc 1.95.0`, `cargo 1.95.0`,
Git 2.50.1 (Apple Git-155).

## Архитектурная проверка и пределы

Изменены только responsibility-named integration tests и документы. Тесты вызывают
существующие public application/composition interfaces и фактический CLI; они не
обходят service/adapters, не добавляют public API, trait, module, runtime, dependency,
schema, storage protocol или recovery abstraction. `src/lib.rs` и adapters после
ручных mutations побайтно восстановлены. Поэтому направления imports, visibility,
composition root и domain/service/adapter ownership product-кода неизменны.

Этап A **не** доказывает полное corruption recovery, удаление и reindex cache/vectors,
reuse здоровых векторов без inference, invalid-current/last-valid сценарий, полную
MC-2 interruption matrix, pack/media corruption, power loss, TK-28 storage criterion,
G-1/G-2, TK-38/39 или готовность пилота. Все критерии Stage B остаются unchecked:
принятая TK-25 ещё не интегрирована в этот branch, а отдельный утверждённый plan
должен зафиксировать её точную полную revision, реальные API и checkpoints.
Никаких reindex stubs, auto-delete/repair, persistent corruption problems, merge,
push, GitHub closure или Engram writes в этапе A нет.

## Принятие этапа A

Whole-branch review `913b2b7..2793f9b` приняло спецификацию этапа A, без
Critical/Important findings. Единственное Minor — три уровня вложенных циклов —
исправлено в `0fcf8bd`; scoped re-review подтвердило сохранение четырёх новых
CLI-процессов и всех assertions, новых findings нет. Final source проверен
focused `corruption_diagnostics` (2/2), fmt и Clippy; полная предыдущая verification
сохраняет свой captured source и не выдаётся за повторный полный запуск на `0fcf8bd`.

Исполнительские rulings: (1) healthy leaf содержит `CORRUPTION_CANARY`, чтобы
anti-leak assertion проверяла реальный ввод; ошибочное решение потребует только
переделки fixture/oracle. (2) запрет production changes означает запрет committed
изменений: узкие временные mutations разрешены с exact restore; цена ошибки —
восстановление локальных source bytes, без изменения TK-25 или продукта.

Execution workspace с полными task reports, review packages и raw logs сохранён
с SHA-256 manifest в `.artifacts/tk26/stage-a-execution-0fcf8bd/`. Ветка `tk-26`
сохранена без слияния/публикации. Для B подготовлен отдельный
[план восстановления](../../../../superpowers/plans/2026-10-26-tk-26-corruption-recovery.md),
который пока требует утверждения.
