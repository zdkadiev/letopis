# Issue #108 Historical Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** читать исторический blob один раз для разных диапазонов одного tree/path в принятом batch, сохраняя свежие текущие наблюдения и проверенную семантику результатов.

**Architecture:** bounded metadata plan в `ProvenanceService`; первое востребованное успешное Git-чтение заполняет результаты группы. Чистая domain-операция вычисляет byte ranges за один проход. Существующий алгоритм остаётся fallback для вызова целиком, если план превышает бюджет.

**Tech Stack:** закреплённый Rust 1.95.0, существующие gix и sha2, synchronous I/O; Python 3.12 и существующие performance utilities для отдельных синтетических измерений.

**Spec:** [согласованная спецификация](../specs/2026-11-10-issue-108-historical-provenance-design.md).

**Статус плана:** согласован к исполнению пользователем 2026-11-10. Способ исполнения по последующему указанию пользователя — subagent-driven; задачи реализуются последовательно с отдельным ревью каждого шага и всего результата.

## Global Constraints

- Область повторного использования — один `observe_sources_batch`, ключ группы `(TreeOid, RepoPath)`; singleton имеет собственный lifetime. `compare_revision` не оптимизировать.
- Первое успешное содержимое группы задаёт её исторические результаты; повторный batch не наследует их. Raw Git reads и strong closure audits продолжают проверять объекты.
- Для каждой локальной привязки выполняется отдельный свежий live capture в прежнем порядке. Будущие группы не читаются заранее; ошибки не переупорядочиваются.
- `HISTORICAL_ENTRY_LIMIT = 4096`: сумма уникальных roots и точных исторических результатов. `HISTORICAL_PLAN_ADDRESS_BYTE_LIMIT = 1024 * 1024`: адресные строки, проверяемые до I/O. Нет нового config.
- План принимается целиком либо весь вызов использует прежний fallback. Переполнение счётчиков считается превышением бюджета, не panic и не усечением входа.
- Отсутствие файла не создаёт групповой кеш; прежний точный `None` не переписывается поздним успехом группы. Недоступные roots перепроверяются.
- SHA-256 и точные bytes без преобразования UTF-8/EOL. Blob буфер освобождается после подготовки группы; payload нескольких групп не удерживается.
- Нет SQL/schema изменений, новых refs/dependencies/runtime, Git CLI в продукте, постоянного кеша, смены хеша или параллелизма.
- Сохранить чужие правки, включая текущий `letopis/development/performance-bottlenecks.md`. Реализацию начать в изолированном worktree через `superpowers:using-git-worktrees`; перенести только документы #108, не коммитить чужие файлы.
- Каждый кодовый шаг проходит red/green цикл; локальные commits группируют завершённые задачи. Push/PR/merge не входят в этот план.

## Review Focus

Пять условий, которые легко упустить в обычном happy path; они закреплены тестами ниже:

1. Файл отсутствовал, затем появился: прежний точный `None` остаётся, новый адрес может получить успешную группу (Task 3).
2. Первая live-ошибка и будущий повреждённый Git-путь: live-ошибка имеет приоритет, будущего чтения нет (Task 3).
3. Корректный диапазон рядом с неполным/огромным: отсутствие не становится пустым валидным срезом, арифметика не переполняется (Task 2).
4. Тысячи повторов и очень длинные адреса: оценка считает уникальные записи, соблюдает entry/byte budgets, fallback не меняет результат (Task 3).
5. Измерение на стандартном корпусе скрывает пользу группировки: отдельно нужны повторные диапазоны одного файла и контроль с отдельными файлами, не менее трёх пар на доступном размере (Tasks 1, 5).

## Файлы и владение

| Файл | Изменение |
|---|---|
| `src/domain/source.rs`, `source_test.rs` | чистое получение byte ranges, семантика диапазонов |
| `src/service/provenance.rs`, `provenance_test.rs` | план, бюджет, ленивое получение группы, fallback |
| `src/service/mod.rs` | документация batch-контракта |
| `tests/provenance_observation.rs`, `provenance_commands.rs` | реальные Git/CLI regression tests |
| `tests/read_verification.rs` | сохранить full-audit guarantees; расширять только при недостающем сценарии |
| `tests/performance/historical.py`, `test_historical.py` (новые) | специализированные inputs/measurement orchestration на существующих utilities |
| `tests/performance/README.md` | воспроизводимые команды новой кампании |
| `docs/zkit/letopis/contracts/I-4-sources.md`, `mechanics.md` | принятая граница наблюдения |
| `docs/zkit/letopis/checks/issue-108/{report.md,commands.md}` (новые) | итоговые результаты и ограничения |
| `letopis/development/performance-bottlenecks.md` | дополнение подтверждённых результатов без потери прежних правок |

Git implementation, `Sha256::digest`, storage, composition и batch caller не требуют изменения поведения. Если реализация обнаружит необходимость менять эти границы, сначала зафиксировать конкретную причину; не добавлять cache в raw reader.

## Task 1: воспроизводимый baseline и специальные входы

**Files:** create `tests/performance/historical.py`, `tests/performance/test_historical.py`; modify `tests/performance/README.md`.

**Interfaces:** CLI `historical.py --before BUILD_JSON --after BUILD_JSON --nodes {10000,100000} --variant {shared,distinct} --stage {timings,profiles} --evidence ABSENT_ABSOLUTE_PATH`. До candidate допускается `--before` без `--after`: записывать baseline-only, не выдумывать comparison. Повторно использовать `scenarios.build_release`, `measure.measure_process`, `profile.record_profile` и существующее завершение owned children. Не расширять ограничения общего `compare.py`, который сейчас принимает только 60/1000 nodes.

Внутренние Python entry points: `prepare_historical_fixture(output: Path, nodes: int, variant: str) -> dict` и `run_campaign(arguments: argparse.Namespace) -> dict`. Fixture result содержит `repository: Path`, `source_tree: str`, `input_sha256: dict`, `counts: dict`; campaign result содержит `outcome` и пути receipts. Tests: `test_shared_fixture_contains_distinct_ranges`, `test_distinct_fixture_uses_one_source_path_per_knowledge`, `test_three_pairs_alternate_sides`, `test_timeout_stops_series_and_preserves_attempt`, `test_profiles_do_not_enter_ordinary_samples`.

- [ ] **1.1. Написать failing harness tests.** Зафиксировать: одинаковый seed даёт одинаковые bytes/tree; `shared` имеет один source path с whole-file и разными диапазонами; `distinct` имеет отдельный source path на знание; обе стороны одной пары получают идентичные inputs/config/request/oracles. Запретить существующий evidence path. При первой timeout/error сохранить attempt и остановить соответствующую серию без retry; interrupted прекращает запуск новых серий. Счётчик fake measurement: три пары дают ровно шесть ordinary calls, направление пары чередуется, profile calls не входят в medians.
- [ ] **1.2. Проверить red.** `cd tests/performance && /opt/homebrew/bin/python3.12 -B -m unittest test_historical`; ожидается failure из-за отсутствующего runner/API.
- [ ] **1.3. Реализовать специализированную кампанию.** Использовать existing fixture utilities для config, requests и release provenance; создать отдельные standalone repositories, не рабочую `.letopis`. Seed 65, 30 знаний/Markdown, fragment size 2000; embeddings disabled. Исторический source — 100 строк с фиксированным содержимым. Для `shared`: один файл `sources/shared.txt`, source selector whole-file для каждого 16-го знания, иначе строка `1 + index % 64`. Для `distinct`: файл `sources/source-<index>.txt` с теми же bytes, один whole-file selector на знание. Знания содержат одинаковую поисковую метку и одну external-связь; IDs стабильны между сторонами. В каждой форме создать собственный complete source tree, записать правильные навигационные destinations и `source_snapshot`; источник существует и совпадает с live bytes. Manifest хранит точные counts, source tree, hashes входов, requests и независимые ожидаемые outputs. Эти формы диагностируют разную source cardinality; их абсолютные времена не выдаются за изоляцию одной переменной.
- [ ] **1.4. Проверить green.** Повторить harness tests. Малый внутренний fixture из 30 знаний проверить независимыми bytes/metadata assertions и реальным CLI `check`/`status`; это smoke, не 10k baseline. Сначала получить по два conditioning calls, затем три ordinary samples каждой доступной baseline формы/размера. Таймаут 120 s на ordinary call, initial publication до 600 s, stage до 1200 s; каждый elapsed/failure сохраняется до следующего вызова. Все сценарии одной стороны используют стабильные отдельные подготовленные repo; копирование и refresh вне измерения.
- [ ] **1.5. До Rust-правок заморозить baseline.** `/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --freeze-build --source "$PWD" --evidence "$PWD/.artifacts/issue-108/before-build"`; использовать отсутствующий путь. Запустить baseline-only stages новой CLI на 10k/100k, shared/distinct. Сохранить исходники harness и hashes; CPU/RSS scope наследовать из `measure_process`. Не использовать PATH Git wrapper для доказательства in-process чтений.
- [ ] **1.6. Commit завершённого harness.** `test(perf): add historical provenance comparison fixtures`; baseline evidence остаётся в новом ignored каталоге.

## Task 2: чистое определение byte ranges

**Files:** modify `src/domain/source.rs`, `src/domain/source_test.rs`.

**Interfaces:** добавить `LocalSource::get_byte_ranges(bytes: &[u8], selectors: &[Option<LineRange>]) -> Vec<Option<std::ops::Range<usize>>>`. `None` во входе означает whole-file; `None` в результате — недоступный полный диапазон. Выход соответствует порядку и длине входа, включая повторы. Типы импортировать через `use`. Существующий `select_bytes` оставить независимым oracle и текущим API.

Опорная assertion теста `batch_ranges_preserve_exact_line_bytes`:

```rust
let selectors = [None, Some(LineRange::new(1, 1).unwrap()),
    Some(LineRange::new(2, 3).unwrap()), Some(LineRange::new(3, 4).unwrap())];
assert_eq!(LocalSource::get_byte_ranges(b"a\r\nb\nlast", &selectors),
    vec![Some(0..9), Some(0..3), Some(3..9), None]);
```

- [ ] **2.1. Написать failing tests.** Literal assertions для `b"a\r\nb\nlast"`: whole-file `Some(0..9)`, L1 `Some(0..3)`, L2 `Some(3..5)`, L2–L3 `Some(3..9)`, L3–L4 `None`. Для пустых bytes: whole-file `Some(0..0)`, L1 `None`. Для `b"a\n"`: L1 `Some(0..2)`, L2 `None`. Проверить бинарные bytes, unsorted/duplicate selectors, `u64::MAX` конец и пустой список. На детерминированных малых byte arrays сравнить каждый полученный slice с независимым `select_bytes`.
- [ ] **2.2. Проверить red.** `cargo test --locked --offline --lib domain::source::`; failure должен указывать на отсутствующий batch API, не ошибочную fixture.
- [ ] **2.3. Реализовать API.** Отсортировать необходимые начала/концы строк и одним проходом найти offsets; не использовать `end + 1` без checked arithmetic, полный per-line index или повторный scan на селектор. Восстановить original order. API чистый, без hashing/Git/IO; документация с `# Returns`.
- [ ] **2.4. Проверить green.** Повторить domain tests; existing `select_bytes` tests должны сохраниться. Временной instrumentation на длинном файле подтвердить один scan независимо от числа селекторов; не превращать wall-time assertion в unit test.
- [ ] **2.5. Commit.** `feat(domain): resolve source ranges in one byte scan`.

## Task 3: bounded plan и ленивое историческое чтение

**Files:** modify `src/service/provenance.rs`, `src/service/provenance_test.rs`, docs для `SourceProvenance` в `src/service/mod.rs`.

**Interfaces:** приватный `HistoricalPlan<'request>` хранит группы со ссылками на входные TreeOid/RepoPath/SourceUri. `HistoricalPlan::build_plan(requests: &'request [ProvenanceRequest<'_>]) -> Option<Self>` возвращает `None` при budget overflow; точные связи lifetimes подобрать без owned clone всего входа. `HistoricalObservations` получает optional plan и сохраняет существующие `available`/`hashes`. Существующие внешние trait signatures не меняются.

Приватный receiver на `ProvenanceService`: `get_historical_hashes_by_tree_and_sources(&self, tree: &TreeOid, sources: &[&SourceUri]) -> Result<Option<Vec<Option<Sha256>>>, SnapshotError>`; все sources одной группы и локальны. Внешний `None` означает отсутствующий blob; внутренние `None` — недоступные селекторы успешно прочитанных bytes. Очерёдность результатов равна sources. Эта операция вызывает Git один раз, Task 2 API один раз, SHA-256 один раз на уникальный валидный selector.

Основные новые тесты: `batch_reuses_history_across_distinct_ranges`, `batch_defers_historical_damage_until_next_read`, `batch_preserves_exact_missing_result_after_group_recovers`, `batch_live_failure_precedes_future_group_failure`, `plan_overflow_uses_whole_call_fallback`. Для четырёх запросов [L1, L2, L1, whole] опорные assertions после успешного batch: `assert_eq!(service.git.reads.get(), 1)` и `assert_eq!(service.filesystem.reads.get(), 4)`; flags проверяются отдельными literal expectations для заданных fixture bytes.

- [ ] **3.1. Написать failing grouped-observation tests.** Расширить existing fake Git/FileReader сценариями: [L1, L2, L1, whole] одного дерева/пути дают один blob read и четыре fresh live reads; разные trees/paths не объединяются; без basis/remote нет нового I/O. После первого live capture fake Git меняется на corrupt/missing/policy error: остальные подготовленные диапазоны используют результаты, новый batch встречает failure. Оставить проверки root recovery и exact-history reuse. Изменить прежний ожидаемый счётчик четыре чтения для двух trees × двух диапазонов на два, сохранив literal flags.
- [ ] **3.2. Написать failing ordering/recovery tests.** Первая live capture падает, следующий path/root повреждён: Git к следующей группе не обращается. Missing blob на L1 сохраняет точный `None`; L2 инициирует уже доступный файл и успешную группу; поздний L1 остаётся `None`. Неполный диапазон в успешной группе не маскирует валидный соседний; missing/invalid root может восстановиться до следующего запроса. Root/blob type/I/O/corruption ошибки сохраняют классификации.
- [ ] **3.3. Написать failing budget tests.** Проверить ровно 4096 root+selector entries и следующий уникальный адрес; много одинаковых запросов не расходуют уникальные entries. Address-byte accounting: сумма `tree.as_str().len() + source.as_str().len()` для каждого уникального `(tree, source)` плюс `tree.as_str().len()` для каждого уникального объявленного root, включая roots запросов без local sources. Ровно 1 MiB допускается, следующий байт запускает whole-call fallback. Учёт консервативно включает повтор tree/path в разных source strings; заимствование его не уменьшает. Проверить длинный путь, границы checked arithmetic, отсутствие partial plan и прежние cache overflow outcomes. Для deterministic overflow арифметики тестировать приватную функцию расчёта размера с `usize::MAX`, без выделения огромной строки.
- [ ] **3.4. Проверить red.** `cargo test --locked --offline --lib service::provenance::`; новые assertions чтений и новой границы наблюдения должны падать на старом поведении, существующие stable-data assertions — проходить.
- [ ] **3.5. Реализовать план без I/O.** Сначала ограниченно посчитать уникальные roots/selectors/адресные bytes; остановить построение сразу при overflow. Groups/boundaries ограничены entries; не собирать сначала unbounded map. Для принятого плана резервировать логическую capacity всех результатов. При `None` оставить существующую последовательную обработку и кеш с прежними ограничениями. Не вводить новый service/interface или service подкаталог.
- [ ] **3.6. Реализовать lazy groups.** В existing historical lookup сначала вернуть уже сохранённый точный результат. Иначе при наличии группы получить все её hashes receiver-операцией, затем вставить результаты через entry-or-existing, сохраняя прежние `None`. При отсутствующем blob сохранить только текущий точный `None` по прежним правилам, не отмечать группу готовой. При успехе заполнить все незаполненные селекторы, отпустить bytes и продолжить existing live path. Нет будущих root/path reads и группового кеша ошибок. Singleton остаётся на прежнем пути: дополнительный plan не нужен для одного запроса.
- [ ] **3.7. Обновить контракт и проверить green.** В `SourceProvenance` описать переиспользование успешного содержимого между разными диапазонами, возможное отложенное обнаружение последующего damage, fresh live captures, порядок ошибок и batch-local scope. Повторить unit tests; для stable inputs сравнить batch с последовательным singleton и literal expected flags. Не требовать равенства при специально инъецированной исторической мутации, где контракт изменён.
- [ ] **3.8. Проверить операции отдельной instrumentation-сборкой.** В изолированной evidence source copy добавить scoped counters на blob reads, decode и historical SHA calls; сохранить exact patch/hash/binary. Для accepted shared group ожидаются один successful blob read/decode на группу и один historical hash на уникальный валидный selector. Считать actual decodes независимо: Git может дополнительно читать trees; не выдавать один method call за доказательство одного decode. Сборку не использовать для ordinary timing; удалить instrumentation из shipping diff.
- [ ] **3.9. Commit.** `perf(provenance): share verified historical bytes across batch ranges`.

## Task 4: реальные Git/CLI гарантии и актуальные контракты

**Files:** modify `tests/provenance_observation.rs`, `tests/provenance_commands.rs`, при необходимости `tests/read_verification.rs`; modify `docs/zkit/letopis/contracts/I-4-sources.md`, `docs/zkit/letopis/mechanics.md`.

**Interfaces:** использовать existing `wire_source_provenance`, `TempRepo`, `FORMATS`, `ReadingRepo`, `create_basis`, `parse_sources`; не открывать внутренние implementation types ради integration tests.

- [ ] **4.1. Добавить/запустить integration regression tests.** Оба Git formats: несколько диапазонов одного исторического файла, разные basis при одинаковом пути, literal expected source_changed/source_not_found. Между двумя batch calls повредить/удалить blob и проверить typed failure/absence следующего фактического чтения; до damage результат чистый. Existing reader tests должны по-прежнему ловить повреждённую замену после успешного прямого чтения. Наблюдение изменения внутри batch детерминированно покрыто Task 3, не использовать flaky sleeps/races.
- [ ] **4.2. Проверить публичные команды.** Shared-range knowledge fixture: `check`, `status`, `search`, `traverse`, `inspect`, `index --full` возвращают ожидаемые результаты; после live source edit флаги меняются без изменения author basis. Сохранить HEAD/index/Markdown bytes и revision expectations. Existing full-audit tests обязаны отклонять unrelated damage до SQL publication. Если current tests уже доказывают конкретный invariant, запускать их, не писать дубликат.
- [ ] **4.3. Запустить целевые проверки.** `cargo test --locked --offline --test provenance_observation --test provenance_commands --test read_verification --test provenance_revisions`; ожидается zero failures. Изменение кода начинается с red там, где вводится новая assertion; уже существующие invariants проходят и до, и после оптимизации.
- [ ] **4.4. Обновить I-4/I-7 и mechanics.** Отметить approved batch observation boundary со ссылкой на спецификацию #108; сохранить архитектурные owners, fresh live/error semantics, полные audits. Старую спецификацию #65 и исторические receipts не переписывать. Проверить локальные ссылки.
- [ ] **4.5. Commit.** `test(provenance): verify grouped history and preserve full audits`.

## Task 5: производительность, оставшееся hashing и завершение

**Files:** create `docs/zkit/letopis/checks/issue-108/report.md`, `commands.md`; update `letopis/development/performance-bottlenecks.md` и статусы шагов этого плана.

**Interfaces:** CLI Task 1, immutable before/after `build.json`, обычные samples отдельно от instrumentation и Samply. Source IDs/input hashes должны объяснять оба build, включая незакоммиченный diff, если он был при freeze.

- [ ] **5.1. Полная проектная проверка.** Выполнить один `make verify V=1` после последних Rust изменений, дождаться завершения и сохранить logs/totals debug/release/docs. Дополнительный storage/native runner не нужен при неизменных storage/native/build/vendor. При Rust-исправлении после проверки повторить затронутые checks и обязательную финальную verify на shipping state.
- [ ] **5.2. Заморозить candidate.** Повторить freeze-build Task 1 с новым absent `.artifacts/issue-108/after-build`; shipping source не содержит instrumentation. Одинаковый rustc/release flags для обеих сторон.
- [ ] **5.3. Выполнить ordinary comparisons.** Для каждого `nodes ∈ {10000,100000}` и `variant ∈ {shared,distinct}` вызвать `historical.py --before <before/build.json> --after <after/build.json> --nodes <nodes> --variant <variant> --stage timings --evidence <new-absolute-path>`. Проверять `check` и `status`; две conditioning calls на сторону, минимум три успешные чередующиеся пары на доступный сценарий. Таймаут/ошибка остаются отдельными records, серия останавливается без auto retry. Ограничения публикации/host не считать speedup. Если 100k недоступен в заданном бюджете, отчёт и issue acceptance остаются явно частичными, не объявлять всю задачу завершённой.
- [ ] **5.4. Получить профили и hashing breakdown.** `--stage profiles` для успешных shared-сценариев на доступных размерах; сохранить raw capture, symbols и CLI-only inclusive stacks. Ordinary wall/CPU/RSS не брать из профилированных запусков. Отдельная instrumentation source copy измеряет Git lookup/decode, поиск boundaries и SHA-256 (historical и live раздельно), включая число ranges и суммарные hash bytes. Сохранить patch, overhead caveat и exact identity. Проверить существующие sha2 features/target CPU support, описать реально использованный backend по проверяемой build/config информации; не менять алгоритм или включать неподтверждённый hardware claim.
- [ ] **5.5. Подготовить отчёт.** Показать baseline/candidate counts и medians, CPU/RSS scope, read/decode/hash counters, budget fallback, unique-file control и оставшиеся hotspots. Нельзя суммировать inclusive sampled scopes. Указать observed regression, если есть; при существенном замедлении control или росте памяти расследовать и скорректировать алгоритм до рекомендации к интеграции, затем повторить только затронутые измерения с новым evidence. Нет заявления production/minimum-hardware acceptance.
- [ ] **5.6. Обновить знания.** Следуя Letopis skill, сохранить прежние незакоммиченные записи, добавить только проверенные выводы с новым source basis после сравнения exact addressed bytes. Выполнить check, search/inspect соответствующей записи и прочитать returned tree bytes; результаты новой проверки не приписывать прежнему source_snapshot.
- [ ] **5.7. Независимый финальный обзор.** Для Native применить `superpowers:requesting-code-review`: свежий reviewer проверяет shipping diff/spec, budget ownership, error order, negative recovery, line-range oracle и evidence. Не переключать модель произвольно: использовать доступного подходящего reviewer согласно действующим правилам. Исправления проверить по затронутой области; не объявлять незавершённый обзор выполненным.
- [ ] **5.8. Завершить локальными commits и отчётом пользователю.** Указать actual changes, tests и измеренный эффект с ограничениями, ссылки на документы, оставшиеся критерии #108. Не push/merge/close issue автоматически.

## Самопроверка плана

- Spec coverage: алгоритм и scopes — Tasks 2–3; errors/negative recovery/budgets — Task 3; Git/CLI/audits — Task 4; замеры/counters/hash backend/knowledge — Tasks 1 и 5.
- Domain API определён в Task 2 и потребляется Task 3; grouped SHA API определён только в Task 3. Публичные trait signatures и dependencies не меняются.
- Entry/byte bounds определены до I/O, fallback атомарен относительно выбора алгоритма. Ни один шаг не требует неограниченно собирать вход перед проверкой budget.
- Пять Review Focus условий имеют именованные assertions в соответствующих задачах. Cancellation использует existing owned-process cleanup плюс проверку отсутствия retained batch state; нового cancellation framework нет.
- Shipping correctness, instrumentation counters, Samply и ordinary timing — отдельные доказательства с отдельными identities. Этот план ещё не выполнен.
