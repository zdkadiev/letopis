# TK-26 — corruption recovery evidence A/B

Дата: 2026-10-26. Карточка **done** после task-scoped и итогового
whole-branch review этапа B. Ниже сохранены исторические evidence этапа A и отдельно
[новые проверки этапа B](#stage-b). Утверждённая
[спецификация](../../../../superpowers/specs/2026-10-26-tk-26-corruption-design.md)
и [план B](../../../../superpowers/plans/2026-10-26-tk-26-corruption-recovery.md)
задают границы. Этапы A и B приняты после независимых reviews.

## Исторический этап A

Следующие разделы до «Этап B» относятся к исходным captured source/logs этапа A.
Они не объявляются повторными проверками интегрированного дерева TK-25.

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
G-1/G-2, TK-38/39 или готовность пилота. На момент завершения A все критерии Stage B оставались unchecked:
принятая TK-25 ещё не была интегрирована в этот branch, а отдельный утверждённый plan
должен был зафиксировать её точную полную revision, реальные API и checkpoints.
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
сохранена без слияния/публикации. После A был подготовлен отдельный
[план восстановления](../../../../superpowers/plans/2026-10-26-tk-26-corruption-recovery.md),
который на тот момент требовал утверждения. Текущее состояние B приведено ниже.


<a id="stage-b"></a>
## Этап B — реальный reindex и MC-2, 2026-10-26

Принятый TK-25 `e8eb1a2` вошёл в main как
`d26533311e1443da01f7368625c9557353bd76f2`; controller интегрировал именно эту
зависимость merge-коммитом `0f980e184a13e6d4054e38d5a234e146f35f84d9`.
Task 1 — `1da09f8e1e499fdd6701843a9a941fae2a7ea3e7`; final B проверяет этот HEAD
плюс новый тест с hash ниже. Исходный A, merged-source baseline и B не смешиваются.

Добавлены только
[corruption_recovery.rs](../../../../../tests/corruption_recovery.rs) и
[corruption_reindex.rs](../../../../../tests/corruption_reindex.rs), а также документы.
Product Rust, tests/support, process harness, schema, native/build inputs и Cargo
зависимости не менялись относительно принятой TK-25. Второго recovery алгоритма,
production unlink/repair или нового error store нет.

### Случай → тест → независимый oracle

| Случай | Тест / oracle | Проверенный исход |
| --- | --- | --- |
| Missing/nonempty-corrupt cache/vectors/both | `reindex_recovers_explicitly_removed_cache_vectors_or_both`; 6 отдельных fixtures, literal a/Alpha, b/Bravo, context edge, files | Corrupt сначала два reindex и два status дают safe internal_error; только явное точное удаление DB/WAL/SHM разрешает rebuild; revision 1 при потере cache, иначе 4, затем +1 |
| Cache loss при здоровых vectors | `reindex_preserves_healthy_vectors_after_corrupt_cache_removal`; буквальные a=[0.25,0.75], b=[0.75,0.25], dimensions=2, прежние hashes/identity | Реальный reindex/reopen дважды сохраняет точные vectors; test-owned embed lock и локальный HttpFixture фиксируют 0 inference requests; whole SQLite-file equality не требуется |
| Требуемый повреждённый author basis: root и child отдельно | `reindex_rejects_corrupt_required_basis_without_advancing`; read-only SQL snapshot всех project/nodes/edges/files полей и FTS term/doc/col/offset, literal healthy IDs/edge/basis | Два новых CLI reindex сохраняют revision 1 и полную старую проекцию; один safe internal_error/exit 1, пустой stderr; exact object restore → реальный reindex revision 2 |
| Invalid A и valid B после потери last-valid cache | `reindex_after_cache_loss_reports_invalid_file_and_publishes_valid_peer`; Alpha/Bravo → malformed A + Charlie/B, затем Delta/A | Новый cache revision 1 содержит только Charlie/B; Alpha/Bravo не изобретаются; files.error и два fresh status показывают safe invalid_metadata; repair/reindex revision 2 содержит Delta/A и Charlie/B, error/problem исчезает |
| Инфраструктурный отказ после restart | Stage A `corrupt_cache_and_vectors_fail_in_each_new_command_process`, новый Git-required test и Task 1 | Новые процессы снова дают hard failure, не false fresh revision:null и не успешно сохранённую corruption-строку |

Сохранность проверяется по текущим canonical Markdown/source bytes, default
staged/unstaged live.md, config presence/bytes, HEAD/index/Git config/user refs.
Git-case дополнительно сравнивает все refs, включая retained snapshots: новый basis
не удерживается. Повреждённые loose bytes не меняются до точного fixture-control
restore. Удаляется только закрытый test-owned cache/vectors и точные WAL/SHM;
canonical parent и regular file проверяются, symlinks не следуются, locks/config
не удаляются. После потери cache старое invalid A из отсутствующей истории не
восстанавливается. Потерянные векторы без inference не объявляются recovered.

### Fresh команды и mutation receipts

Все build/test/Clippy/verify запуски последовательны, с
`CARGO_TARGET_DIR=/private/tmp/letopis-tk26-task1-target`, `CARGO_BUILD_JOBS=2`,
`RUST_TEST_THREADS=2`, `CARGO_INCREMENTAL=0`; Python `/opt/homebrew/bin/python3 -B`.
Host: Darwin 25.6.0 arm64, pinned Rust/Cargo 1.95.0. Новые полные логи Task 2:
`.artifacts/tk26/stage-b-task2-20261026/`; Task 1 — отдельный
`.artifacts/tk26/stage-b-task1-20261026/`.

| Команда / stage | Exit и фактический результат | Новый log |
| --- | --- | --- |
| Task 1 merged baseline: Git/storage/diagnostics/reindex/document_reindex | 0; 22 passed | Task 1 initial-regression.log |
| Task 1 restored recovery/reindex/document_reindex | 0; 14 passed | Task 1 restored-focused.log |
| Task 1 make check V=1 | 0; 774 outer passed/0 failed/5 ignored; 8 дополнительных child summaries не считаются второй раз | Task 1 make-check.log |
| Первый `cargo test --locked --offline --test corruption_reindex` | 101; Git case GREEN, invalid case runtime mismatch ошибочно заданного test literal, не product gap | first-reindex.log |
| `cargo test --locked --offline --test corruption_reindex --test corruption_diagnostics --test provenance_commands --test reindex_command` | 0; 17 passed (2+2+6+7), 0 ignored | focused.log |
| `cargo test --locked --offline --lib service::document_refresher::tests::process -- --nocapture` | 0; 13 passed/1 ignored/295 filtered; реальные matched schedules | mc2-process.log |
| `cargo test --locked --offline --test document_refresh --test document_reindex --test reindex_command` | 0; 28 passed (16+5+7), 0 ignored | mc2-integration.log |
| `cargo clippy --locked --offline --test corruption_reindex -- -D warnings` | 0 | clippy.log |

Первый invalid-case ошибочно ожидал `invalid metadata`; существующий contract даёт
`invalid letopis metadata`. Исправлен только test literal, новый алгоритм не
понадобился. Это исправление oracle setup; оба сценария характеризуют существующее
поведение GREEN, не выдуманный feature RED.

Required mutation: в `decode_file` после проверки persisted invariants временно
заменено поле `error` на `error: None`. Команда
`cargo test --locked --offline --test corruption_reindex reindex_after_cache_loss_reports_invalid_file_and_publishes_valid_peer -- --exact`
скомпилировалась и дала exit 101 на covering assertion: `None` вместо
`Some("invalid letopis metadata")`. После exact restore та же команда дала exit 0,
1 passed/1 filtered. Source `src/storage/files.rs` original/restored SHA-256
`ffaac4ddfa7cb0e514a37e1665115933c7d27915dcb91d6a4286da3e73157cec`, mutant
`756d5fe9ad9edd4c00b3ff0adb56f2fc2744056c5359b13269037738d7f2836a`.
Логи: diagnostic-mutation.log, diagnostic-restored.log, diagnostic-mutation-hashes.txt.
Task 1 vector mutation/restoration остаётся в своём task report; source read.rs
restored hash `33fa0d9abfdb14883eba62af13bc8b4e645add25a3f73f2e468dbee205a16949`.

### MC-2: соответствие фактическим controls

Все test names ниже имеют prefix
`service::document_refresher::tests::process::`; child module называется `reindex`,
а его source — `src/service/document_refresher/tests/reindex_process.rs`.

| MC-2 условие | Существующий выполненный test |
| --- | --- |
| Reindex SIGINT до/после commit | `reindex::reindex_sigint_preserves_exact_old_or_complete_new_state`: locked/begun/fts/before_commit/after_commit; старое либо целиком новое project/nodes/edges/files/FTS, fresh reindex после released kernel lease |
| Reindex WAL reader | `reindex::reindex_keeps_pinned_wal_view_and_publishes_one_complete_new_view`: pinned old view, complete new view после commit |
| Contending publisher и redetection | `reindex::publisher_waits_for_reindex_and_redetects_latest_file_bytes`: явный contended barrier, latest Charlie bytes и revision 3 |
| Late SQL rollback/released lock | `reindex::reindex_late_sql_error_rolls_back_all_state_and_releases_lock`: old exact state, следующий child успешно публикует |
| Остальные mutation phases refresh | `sigkill_keeps_exact_old_or_complete_new_publication` в process.rs: detected/locked/redetected/begun/cleared/projections/fts/fingerprints/before_commit/after_commit; второй файл реально заменяется; следующий refresh заново обнаруживает delta |
| Contention, owner death, normal composition | `duplicate_publishers_commit_once_after_real_contention`, `waiting_publisher_redetects_newer_edit_after_owner_is_killed`, `normal_open_waits_at_index_lock_during_cache_write_then_redetects`, `normal_open_redetects_after_active_cache_writer_is_killed` |
| Normal WAL, rollback и noop | `pinned_wal_reader_keeps_old_snapshot_and_new_reader_sees_complete_commit`, `late_storage_error_rolls_back_every_cache_mutation`, `noop_finishes_while_another_process_holds_index_lock`, `normal_open_and_noop_finish_during_vector_write_and_held_index_lock` |

Ignored `process::run_child` — entry point, который Worker действительно запускает
через `--exact ... --ignored --nocapture` с Unix-socket checkpoints/watchdogs.
Нулевой selected result не используется как PASS. Новых sleeps/checkpoints нет.
Signal death не равен TK-29 handled cancelled JSON и не является физической потерей
питания или доказательством persistence на носителе.

### Финальная проверка B и handoff

Единственный финальный `make verify V=1` дал **exit 0**, terminal `ok`:
fmt/check/all-target Clippy, strict Rustdoc и root debug/release прошли. В каждом
профиле **776 outer passed / 0 failed / 5 existing ignored** в **59 executable
targets**. Последняя outer summary каждого Cargo Running target считается один
раз; 8 nested child summaries в каждом профиле исключены, 0-test Doc-tests тоже.
`count-outer-tests.py` и outer-test-counts.log/json сохраняют воспроизводимый подсчёт.
Это новый запуск на final B inputs, отдельно от исторического Stage A.

Полный make-verify.log: 2 395 строк, SHA-256
`9a138836c8180b19e4df8317669f2fad5ed30d525cc985976d58c82792b69771`.
Существующий MC-2 harness пишет child evidence в default .artifacts/tk10/process;
270 новых log/receipt files этого task скопированы без изменения оригиналов в
mc2-child-evidence нового каталога B, с hashes/paths в mc2-child-copy-manifest.txt.
Пять ignored root entries — три entry points, реально вызываемые parent schedules,
и отдельные real deadline/external corpus acceptance probes, не запущенные здесь.
Новых ignored tests нет.

Финальные `git diff --check` и local Markdown checker дали exit 0: 44 local links,
10 anchors разрешаются. Сохранённый command:
`/opt/homebrew/bin/python3 -B .artifacts/tk26/stage-b-task2-20261026/check-markdown-links.py`.
Первый lightweight checker ошибочно прочитал fixture-link внутри inline code как
ссылку документа; после исключения code blocks проверка зелёная, валидные ссылки
и fixture не подменялись. Report/card/spec/plan согласованы: execution evidence
готово, независимые controller reviews остаются pending.

`source-input-sha256.txt` фиксирует product Rust/SQL, shared helpers, Cargo/toolchain,
build.rs/Makefile и релевантные integration inputs; SHA-256 manifest
`af3421dd5a314fb8098a7df7e92b8f857bcfd16ef4af305a4af436a5bd66544d`.
Task 1 test hash `9b92d546eac56c7df073456195eab001abbafd3aa6f0f57bc1cfb6fa8f9dce10`;
Task 2 test hash `487b95c8bb1e4e6fa895e528b485001fac5aafb22e7188f1e1b4cf65fe236441`.

Архитектурная self-review: тестовые файлы с private helpers; product visibility,
traits/impls, imports и lib.rs composition не менялись. Реальный CLI потребляет
существующие services/adapters; test-owned read-only SQL — независимый oracle, а
Storage API проверяет files/FTS. Domain не получает I/O. Product diff после mutations
пуст относительно принятой TK-25. Независимые task/final reviews выполняет controller.

[TK-28 storage-failure handoff](../../roadmap/epics/EP-3-maintenance/tasks/TK-28-diagnostics.md):
эти tests различают безопасный infrastructure error и сохранённую files.error;
при cache loss fresh status не создаёт БД, после rebuild invalid diagnostic текущий
и исчезает после repair. Это evidence для storage-критерия TK-28, а не закрытие всей
карточки: составной query/drain lifecycle остаётся отдельной приёмкой.

Linux/native runner на B не запускался: accepted product/storage/native/build inputs
не менялись, добавлены только tests/docs. Paid/live inference, отдельный real
60-second deadline и external corpus acceptance probes не запускались. Нет новых
claims о media/pack corruption, power loss, gates G-1/G-2, пилотах, TK-29 или TK-38/39.
Ветка сохранена без main merge/push/GitHub closure. Engram и review ledger — controller.

## Итоговая приёмка A/B

Task 1 (`0f980e1..1da09f8`) и Task 2 (`1da09f8..70a4fdf`) прошли независимые
spec/quality reviews. Minor в отчётном подсчёте Task 1 исправлен: 774 outer passes
плюс 8 вложенных child summaries, без повторного выполнения и без изменения кода.
Итоговое whole-branch review `d265333..70a4fdf` принято, Critical/Important/Minor — 0.
Контроллер независимо сверил 152 source/input hashes, точное восстановление mutations
и полный verify log: debug/release по 776 passed / 0 failed / 5 ignored, 59 executable
targets на профиль, вложенные summaries не посчитаны повторно. После verification
изменены только completion-документы; исходники тестов/продукта прежние.

Отложенные ревьюером Linux/native, historical standalone harnesses, power loss,
media/pack, gates/pilots, handled cancelled JSON, regeneration потерянных embeddings,
autorepair и утраченное invalid-file history соответствуют явной границе утверждённого
плана; новые исключения из критериев TK-26 не вводились.

Решение исполнителя (Ruling): запрет merge относится к публикации tk-26 в shared main;
потребление закреплённой принятой TK-25 в собственной tk-26 разрешено Task 1.
Цена неверного толкования — переделка одного неопубликованного локального merge.

Execution ledger, task reports и review packages сохранены с checksum manifest в
`.artifacts/tk26/stage-b-execution-70a4fdf/`; raw evidence остаётся в отдельных
stage-b-task1/task2 directories выше. Ветка/worktree сохранены без main merge,
push и GitHub closure. Результат записывается в существующий Engram store.
