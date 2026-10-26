# TK-27 — embedding lifecycle verification

2026-10-26. Реализация и scoped reviews Tasks 1–2 завершены; финальная проверка
Task 3: PASS на macOS arm64 и Linux arm64. **TK-27 завершена**: независимое
[итоговое ревью](review.md) принято без замечаний на `02b9736`. [Спецификация](../../../../superpowers/specs/2026-10-26-tk-27-embedding-lifecycle-design.md),
[план](../../../../superpowers/plans/2026-10-26-tk-27-embedding-lifecycle.md),
[карточка](../../roadmap/epics/EP-3-maintenance/tasks/TK-27-embedding-lifecycle.md)
и [машиночитаемый receipt](verification.json) задают scope и provenance.

## Результат и точный source

Task 1 `c668ec617bf911811a984db52bd3b7b5d3f95872` добавляет условный reset через
наблюдавшуюся identity, typed conflict и механически мигрирует все storage callers.
Task 2 `14d1e4773974030430936e2aff3f76138dd5e1ac` добавляет process SIGINT/restart,
cache-loss/reuse и long-input rejection. Условный алгоритм определён один раз в
[MC-3](../../mechanics.md#mc-3--embeddings-работа-из-данных-и-попытки);
X-2/I-8 и spec ссылаются на него. Новых runtime API, schema/dependency изменений,
версий состояния, очередей заданий, блокировок конфигурации или циклов reset нет.

Оба финальных runner используют точный product source `14d1e4773974030430936e2aff3f76138dd5e1ac`.
`make fmt` выполнен до их старта и не изменил Rust/build inputs. Dirty paths при
захвате source — только текущие documentation/model/plan изменения; их перечень
сохранён в `source-dirty.log`. Последующий report commit меняет только docs и
Python-модель, поэтому не объявляется новой скомпилированной ревизией. Полные
по-файловые SHA-256 находятся в runner manifests; verification.json содержит
их digest, общие/обязательные inputs, command results и hashes evidence. Исходный
baseline `207dc1d`: 776 passed / 0 failed / 5 ignored
([исторический лог](../../../../../.artifacts/tk27/baseline-207dc1d/check.log));
он не выдаётся за повторную проверку финального дерева.

## Критерии → выполненные сценарии → независимые oracles

Все перечисленные Rust-сценарии выполнены финальным `make verify V=1` в debug и
release на обеих проверенных платформах; фокусные TDD/mutation receipts Tasks 1–2
использованы в их исходной области, без повторного запуска тех же scoped suites.

| Критерий / риск | Сценарий | Проверяемый результат |
| --- | --- | --- |
| Канонический JSON, реальная смена config | `embedding_provider::ignores_object_order_for_identity`, `changes_identity_when_retries_or_unknown_parameters_change`; `cli_drain::formatting_configuration_keeps_identity_and_does_not_spawn`; `embedding_service::changed_configuration_resets_vectors_but_equal_identity_keeps_them` | Пробелы/порядок сохраняют identity, vector/failure и не запускают HTTP; изменение полей сбрасывает state |
| P(A)/Q(B)/late-P | `service::embedding::tests::process::configuration::stale_configuration_observation_preserves_current_vectors_and_attempts` | Четыре child actions: drain/query/readiness/read_configuration; B identity, literal `[4,5,6]`, attempts=1, cache unchanged и 0 A HTTP |
| Начальное отсутствие, конкурирующие и одинаковые desired identity | Все четыре `storage_configuration` tests | None→A и A→B commit; observed A/None против B conflict; desired B сохраняет exact vector и failure bytes при observed B/A/None; malformed metadata typed error |
| Rollback и старый pinned view | `storage_configuration::reset_commits_only_for_the_observed_identity_and_keeps_a_pinned_read`; `storage_vectors::dimension_reset_and_rollback`; `storage::sqlite::tests::reset_late_meta_failure_preserves_identity_key_vector_and_query_error`; весь `search_consistency` | Старый read сохраняет точные A-vector/failure; schema/meta failure не публикует частичный reset; FTS и read view не подменяются |
| Отсутствующий/invalid/unreadable config и конфликт | `embedding_service::missing_and_invalid_configuration_preserve_corpus_and_prior_vectors`, `unreadable_configuration_returns_safe_invalid_state`, `valid_shape_with_invalid_endpoint_does_not_reset_or_compute`; search/traverse scoped tests | Нет разрушительного fallback/provider calls; FTS/graph доступны, consumer pin один |
| Поздний success/error и изменённый текст | `service::embedding::tests::process::reset_during_http_discards_late_document_success_and_error`; `embedding_service::late_old_query_success_or_error_cannot_replace_new_query_state`; `cli_drain::text_publication_during_http_never_reuses_a_late_vector_for_new_text`; `storage_failures::stale_identity_cannot_reserve_or_record_or_store` | Старые результаты/ошибки не меняют активное state; прежний hash не представляет новый текст |
| Cache deletion/reindex reuse | `reindex_command::reindex_reuses_vectors_and_only_requests_the_missing_hash` и retained no-wait case | a.md остаётся `[1,0,0]`; только BravoToken отправлен один раз с truncate:false; b.md становится `[0,1,0]` |
| Durable attempts, bounded retries и restart | `embedding_service::retries_are_additional_and_exhaustion_survives_restart`; `storage_failures::reservation_is_durable_only_after_commit_and_survives_reopen` | max_retries=0→1 total, 3→4; committed reservations переживают reopen, exhausted не возобновляется при той же identity |
| Реальный SIGINT после reservation | `cli_drain::sigint_preserves_attempt_budget_across_product_restarts` | Первый actual drain получает SIGINT и reaped; lock свободен, attempts=1; второй process оставляет attempts=2, третий не делает HTTP; corpus unchanged |
| Длинный input и safe error без усечения | `embedding_lifecycle::over_context_rejection_keeps_full_text_searchable_and_exhausts_one_attempt` | Полный literal input с последним EndOfLongKnowledgeToken отправлен с truncate:false; HTTP400 даёт exact safe failure, attempts=1/no vector; final marker находится в FTS; повторный drain молчит; canary не утёк |
| Архитектура и интерфейсы | Actual `207dc1d..14d1e47` diff review; all-target check, Clippy, strict Rustdoc | Границы и проверки описаны ниже; новых CLI JSON состояний нет |

## Команды и платформы

Evidence roots (локальные ignored receipts, не Git-tracked raw logs):

- Host: `.artifacts/tk27/final-host-14d1e47-20261026/`.
- Linux PASS: `.artifacts/tk27/final-linux-14d1e47-20261026-escalated/`, inner receipt `container/`.
- Linux initial infrastructure FAIL: `.artifacts/tk27/final-linux-14d1e47-20261026/`.
- Task 3 model/format/validation: `.artifacts/tk27/task3/`.

| Команда | Наблюдаемый исход |
| --- | --- |
| `make fmt` | Exit 0; Rust/build source unchanged |
| `python3 -B docs/zkit/letopis/mechanics/model_embedding.py` | Нормальная модель: 0 нарушений; отдельный observed_identity negative control теряет exact B-vector, failure attempt и identity |
| `python3 -B docs/zkit/letopis/checks/mechanics/check_mechanics_artifacts.py` | Exit 0; три модели, 15 прежних изолированных защит обнаружены; исторические receipts не переписаны |
| `python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk27/final-host-14d1e47-20261026` | PASS: macOS/Darwin arm64, по 785 passed / 0 failed / 5 ignored в debug и release; Python runner tests 10/0; schema/native/standalone/linkage PASS; отдельный real deadline 1/0, 60.01s |
| `python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk27/final-linux-14d1e47-20261026` | FAIL до сборки: sandbox запретил Docker buildx запись `~/.docker/buildx/activity`; product tests не запускались |
| `python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk27/final-linux-14d1e47-20261026-escalated` | PASS с разрешённым Docker access: Linux arm64/glibc2.36, по 785 passed / 0 failed / 5 ignored в debug и release; Python runner tests 10/0; schema/native/standalone/linkage PASS; отдельный real deadline 1/0, 61.86s |
| Documentation links + `git diff --check` | Exit 0; 153 local links (семь Markdown files), MC-3 anchor и diff whitespace проверены |

Runner сам включает один `make verify V=1` (fmt/check/all-target Clippy/root debug
и release tests/strict Rustdoc), затем explicit ignored real-deadline test,
native capability, copied standalone executable и проверку динамической линковки.
Второго одинакового broad gate не запускали. Linux runtime работает с
`--network none`; локальные HTTP fixtures не вызывают реальную модель.
Все 346 host inputs совпали с Linux outer manifest и текущими байтами исходников после обоих runner; 228 общих inputs совпадают с Linux inner manifest. Все 228 inputs Linux inner receipt совпали с outer
source manifest; обязательные `tests/embedding_lifecycle.rs`, `tests/cli_drain.rs`,
`tests/reindex_command.rs`, `tests/support/cli.rs`, service process module и storage
configuration tests включены в Docker context/manifest. Новые сценарии имеют `ok`
в обоих профилях; это проверено отдельно от наличия файлов в manifest.

### Каждая ignored запись и фактически выполненные parent scenarios

Root totals используют последний test-result каждого из 61 root executable на профиль; восемь вложенных helper success entries исключены из суммы. Пять ignored entries в каждом root profile не объявляются пятью отсутствующими
сценариями и не прибавляются к executed parent counts:

| Ignored entry | Фактический scope |
| --- | --- |
| `filesystem::local::tests::run_lock_child` | Private child entry; родительские lock tests выполнены и запускают его явно |
| `service::document_refresher::tests::process::run_child` | Private child entry; production schedule parents выполнены |
| `service::embedding::tests::process::run_child` | Private child entry; старые race parents и новый P(A)/Q(B) parent выполнены; четыре новых actions не считаются четырьмя standalone root tests |
| `attempt_obeys_real_sixty_second_deadline` | Пропущен root gate и отдельно явно выполнен runner один раз на платформу |
| `observe_existing_corpus` | Не выполнен: нужен внешний LETOPIS_CORPUS_REQUEST; не доказывает TK-17/G-1 или actual model quality/capacity |

## TDD и мутации: сохранённые scoped receipts

Task 1: [baseline regression red](../../../../../.artifacts/tk27/task1/regression-red.log)
скомпилировался и упал на разрушенном B state, затем
[green](../../../../../.artifacts/tk27/task1/regression-green.log) прошёл.
В disposable source copy отдельное удаление observed rejection и same-desired
return приводит к exact vector failure (`mutation-observed-guard-vector-red.log`,
`mutation-same-desired-guard-vector-red.log`); byte restoration и четыре storage
checks green (`mutation-final-restored-green.log`). Actual worktree не мутировал.

Task 2: новые missing-evidence сценарии проверяют уже принятую реализацию; initial
SIGINT и long-input runs green. First reindex run выявил только слишком строгое
ожидание необязательного dimensions в fixture; исправлено ожидание, а не product.
[Mutation receipt](../../../../../.artifacts/tk27/task2/mutation-results.json):
бюджет +1 пойман attempts 3≠2, удаление final FTS token поймано hits 0≠1,
удаление matching-identity return поймано лишним AlphaToken HTTP. Все три mutations
скомпилировались, дали exit101 на нужных assertions; exact restore дал exit0.
Их SHA-256 соответствуют финальному source; дополнительный broad suite не нужен.

Полные worker reports и ревью сохранены с прежними именами в
`.artifacts/tk27/sdd-final/task-1-report.md` и `task-2-report.md`; полный ledger и финальное ревью находятся в том же архиве.
Итоговый reviewer подтвердил соответствие спецификации, качество и готовность
к слиянию в scope TK-27. Независимая controller-сверка сохранена отдельно в
`.artifacts/tk27/controller-audit.json`.

## Архитектурная проверка фактического diff

Проверены physical files, visibility/import direction, trait implementations и
composition. `src/service/storage.rs` владеет typed conditional API;
`src/service/embedding.rs` получает/отпускает короткий observation read до file
parse, оркестрирует вызов через trait и возвращает finite Changed без retry.
`src/storage/vectors.rs` владеет BEGIN IMMEDIATE, typed identity parsing и
атомарным reset; `src/storage/read.rs` использует тот же private reader для
attached schema. Storage не импортирует filesystem/config parser/embedding
adapter и не оркестрирует другие adapters. `src/domain/` не менялся.

Миграции `Storage` forwarders и reset fixtures в check/search/traverse/status,
document_refresher, tests/support, target_corpus и standalone example используют
actual observation для намеренных replacement resets; None сохранён для fresh
initialization или явных absence cases. `lib.rs::wire_embedding_executor` и
`wire_embedder` по-прежнему создают EmbeddingService с concrete storage/files/parser/
provider через application-owned interfaces; lib.rs и handlers не менялись.
Consumers сохраняют readiness-before-pin и один scoped view. HTTP вне write unit;
vector/failure publication проверяет captured identity отдельно. Public storage
ошибки остаются typed/safe, runtime payload/credentials не включаются в JSON.
Новых dependencies/schema/runtime/background components нет.

Открытых product findings нет. Итоговое независимое ревью принято;
последующие изменения статуса и сохранённый текст ревью меняют только документацию. Локальные проверки не устанавливают атомарность
config-файла с SQLite, lifetime retry budget через reset/deletion, физический
power-loss durability, производительность/качество реальной модели или все
DR-5/DR-8. G-1/TK-17 не подтверждены; G-2, TK-28, handled cancellation/TK-29,
TK-37 и системные drills остаются отдельной работой. Плановое разрешение на
локальную реализацию не закрывает эти project gates.
