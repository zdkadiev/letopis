# Упрощение типов транзакций Storage — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Статус:** реализация завершена; host verification PASS. Ограничения ресурсной приёмки указаны ниже.
Пользователь уточнил scope Read<'a>/ассоциированных типов и дважды явно поручил
выполнение: «Окей. Выполняем» и «Выполняй».


**Goal:** заменить storage GAT обычными интерфейсами возвращаемых транзакций
и убрать ставшие ненужными параметры хранилища у сервисов.

**Architecture:** Storage возвращает Box<dyn ReadTransaction + '_>,
Box<dyn CacheWrite + '_> и Box<dyn VectorWrite + '_>. ReadTransaction объединяет
существующие StorageRead и GraphRead. SQLite по-прежнему владеет реальными
rusqlite::Transaction<'a>; время жизни возвращённого объекта привязано к &mut self.
Все интерфейсы остаются в service/mod.rs, реализации — в существующих файлах.

**Tech Stack:** закреплённый Rust 1.95.0, std trait objects/Box, существующие
rusqlite/SeaQuery/vec1; новых зависимостей нет.

**Spec:** [обсуждаемое предложение](../specs/2026-11-01-service-types-simplification.md), вариант B.

## Global Constraints

- CLI/JSON, persisted schemas, revision/identity правила и synchronous I/O неизменны.
- Rust API меняется намеренно; compatibility aliases и миграция данных не нужны.
- Сохранить эксклюзивное заимствование, pinned revision, rollback при drop/ошибке,
  poisoning write unit, consuming commit и освобождение handles на ранних выходах.
- Concrete SQLite transactions сохраняют необходимые lifetime-параметры.
- Один Box на выдаваемую транзакцию — осознанная цена стирания её конкретного типа.
  Не добавлять Arc, factories, callback layers или forwarding wrappers для Box.
- FileSystem::IndexLock и EmbeddingFiles::EmbedLock этим планом не меняются.
- Обновлять существующие test wrappers; не создавать параллельную тестовую архитектуру.
- Перед реализацией выполнить `rg -n -e 'type (Read|CacheWrite|VectorWrite)' -e 'begin_(read|status_read|cache_write|vector_write)_tx' src tests examples`
  и зафиксировать полный список callers/wrappers; перечитать живое дерево и сохранить несвязанные изменения,
  особенно текущие изменения storage/read.rs, schema.rs, sqlite.rs и basis.rs.
- Коммиты, push, merge и публикация требуют отдельного указания пользователя.

## Review Focus

- Транзакция остаётся привязана к storage; второй mutable borrow до её завершения
  и использование после drop storage запрещены компилятором.
- ConfigChanged возвращается до выдачи writer, без потери текущих vectors/failures.
- Ошибка или drop boxed writer откатывают все изменения, а commit потребляет writer.
- Status сохраняет None для отсутствующей публикации, не создаёт БД и не расширяет
  свой read-контракт до ненужного GraphRead.
- Изменение ownership/dispatch сохраняет исходные scopes: write transaction и
  embedding lock не охватывают HTTP; существующий pinned search read может охватывать
  query inference ради согласованной выдачи. Сохраняются используемые Send/thread
  сценарии и тестовые Rc.

## Целевой контракт

Существующие методы StorageRead/GraphRead и операции записи сохраняют смысл:

```rust
pub trait ReadTransaction: StorageRead + GraphRead {}
impl<T: StorageRead + GraphRead> ReadTransaction for T {}

pub trait Storage {
    fn begin_read_tx(&mut self)
        -> StorageResult<Box<dyn ReadTransaction + '_>>;
    fn begin_cache_write_tx(&mut self)
        -> StorageResult<Box<dyn CacheWrite + '_>>;
    fn begin_vector_write_tx(&mut self, expected: &Sha256)
        -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>>;
    // reset_vectors сохраняет существующую сигнатуру.
}

pub trait StatusStorage {
    fn begin_status_read_tx(&mut self)
        -> StorageResult<Option<Box<dyn StorageRead + '_>>>;
}
```

CacheWrite и VectorWrite лишаются ограничения Sized. Завершение записи:
`fn commit(self: Box<Self>) -> StorageResult<u64>` для cache и
`fn commit(self: Box<Self>) -> StorageResult<()>` для vectors.
ReadTransaction и его пустой blanket impl — единственное объединение read-контрактов,
необходимое для trait object; новых структур/файлов для него не требуется.

### Task 1: Обычный read-интерфейс вместо Read<'a>

**Files:** src/service/mod.rs; src/storage/sqlite.rs, read.rs, status.rs;
src/service/check_problems.rs, graph_diagnostics.rs, graph.rs, search/*.rs,
status.rs и остальные фактические потребители read views;
существующие read wrappers в service/check/tests*, service/search/tests/scoped.rs,
service/traversal/tests/traced_storage.rs, document_refresher/tests/reading.rs,
service/status/tests.rs; tests/storage_cache.rs, storage_status.rs, graph_read.rs,
search_consistency.rs и их существующие helpers.

**Interfaces:** produces ReadTransaction и две целевые read-сигнатуры выше;
cache/vector write associated types пока остаются для независимой сборки этапа.

- [x] До первого изменения API сохранить baseline release measurements одинаковых
  read, cache-write commit/drop и vector-write сценариев на одной synthetic fixture:
  одинаковые число итераций/прогрев, минимум пять повторов, exact inputs, окружение,
  raw timings и p50/p95. Не смешивать замер с параллельной сборкой или provider.
- [x] Добавить `boxed_read_keeps_graph_and_storage_contracts` в tests/storage_cache.rs:
  Given — опубликован один `a.md` на revision 1; When — результат begin_read_tx явно
  присваивается Box<dyn ReadTransaction + '_>; Then — revision == 1, get_nodes
  возвращает ровно `a.md`, get_node_ref_by_id на `a.md` возвращает его graph reference.
  Увидеть RED компиляции старого API (ReadTransaction/нужный return type отсутствует).
  В storage_status.rs добавить `boxed_status_read_does_not_create_missing_publication`:
  явный Option<Box<dyn StorageRead + '_>>, результат None, cache.db/vectors.db
  отсутствуют и после вызова. Это проверка нового возвращаемого контракта.
- [x] Добавить compile_fail примеры `cannot_borrow_storage_twice` и
  `cannot_drop_borrowed_storage` в документацию Storage: попытка второго mutable
  borrow при ещё используемой read transaction; попытка drop storage перед её
  последующим использованием. Зафиксировать именно borrow/lifetime ошибки.
- [x] Убрать только read GAT; обернуть существующий SqliteRead в Box на границе
  SqliteStorage/SqliteStatusStorage. Сохранить None/ошибки и pinned metadata.
- [x] Передавать view.as_mut() в потребителей. Где нужен только StorageRead или
  GraphRead, использовать &mut dyn соответствующего интерфейса. GraphDiagnostics::
  find_problems принимает &mut dyn StorageRead вместо параметра R; адаптировать
  collect_check_problems и реальную реализацию без новых прослоек.
- [x] Заменить type Read в wrappers на поле Box<dyn ReadTransaction + 'a> либо
  Box<dyn StorageRead + 'a> по фактической роли. Сохранить events/error injection.
- [x] Выполнить cargo check --locked --offline --all-targets и focused tests:
  cargo test --locked --offline --test graph_read;
  cargo test --locked --offline --test search_consistency;
  cargo test --locked --offline --test storage_status;
  cargo test --locked --offline --doc. Ожидание: PASS.
- [x] Для новых lifetime/поведенческих проверок применить компилируемую семантическую
  мутацию где возможно; для compile_fail примеров временно убрать конфликтующий
  borrow/drop и убедиться, что compile_fail перестаёт проходить. Восстановить байты.

### Task 2: Обычные write-интерфейсы и object-safe Storage

**Files:** src/service/mod.rs; src/storage/sqlite.rs, cache_write.rs, vector_write.rs;
src/service/document_refresher.rs, projection.rs и callers commit;
src/service/document_refresher/tests/basis.rs; tests/storage_cache.rs,
storage_vectors.rs, storage_failure_atomicity.rs, storage_failures.rs,
storage_configuration.rs, storage_embedding_work.rs и существующие helpers.

**Interfaces:** consumes Task 1 read methods; produces полный контракт Storage выше,
object-safe CacheWrite/VectorWrite и consuming boxed commit.

- [x] В tests/storage_cache.rs добавить `boxed_storage_commit_drop_and_identity_guards`
  с явным &mut dyn Storage: commit добавления `a.md` возвращает revision 1;
  добавление `b.md` с последующим drop writer оставляет только `a.md` и revision 1;
  после reset к identity digest(b"model-a") попытка writer с digest(b"model-b")
  возвращает ConfigChanged. Увидеть RED старого non-object-safe Storage API.
  В существующих poisoning tests явно работать через boxed writer, сохранив
  независимые literal assertions для полной отмены ранее успешных mutations.
- [x] Убрать CacheWrite<'a>/VectorWrite<'a> из Storage и implementations; убрать
  Sized, заменить commit receiver на self: Box<Self>. Внутренняя SQL-логика,
  poisoning и порядок commit/drop остаются у прежнего владельца.
- [x] SqliteStorage оборачивает только Current(writer); ConfigChanged и ошибка
  проходят без writer/повторной identity-проверки. Concrete begin_tx сохраняются.
- [x] BoundaryWrite и другие существующие wrappers владеют boxed capability,
  сохраняют injection/event порядок и потребляют inner при commit. Согласовать
  изменения basis.rs с уже имеющимися пользовательскими правками, не заменять файл.
- [x] Обновить все вызовы и helpers атомарно; не вводить blanket forwarding impl
  StorageRead/CacheWrite/VectorWrite для Box ради прежних generic signatures.
- [x] Выполнить cargo check --locked --offline --all-targets и focused suites:
  cargo test --locked --offline --test storage_cache;
  cargo test --locked --offline --test storage_vectors;
  cargo test --locked --offline --test storage_failure_atomicity;
  cargo test --locked --offline --test storage_failures;
  cargo test --locked --offline --test storage_configuration;
  cargo test --locked --offline --test storage_embedding_work. Ожидание: PASS.
- [x] Мутации новых сценариев: преждевременный commit при drop, пропуск poison
  либо выдача writer при несовпадающей identity. Каждая применимая мутация должна
  компилироваться и быть отвергнута соответствующим тестом; восстановить байты.

### Task 3: Убрать ставшие ненужными параметры хранилища у сервисов

**Files:** src/service/check.rs, search.rs, embedding.rs, document_refresher.rs,
traversal.rs, status.rs; src/lib.rs; соответствующие существующие unit fixtures
и tests/support/*.rs. GraphDiagnosticsService остаётся конкретной реализацией.

**Interfaces:** consumes object-safe Storage/StatusStorage/GraphDiagnostics;
сохраняет методы Checker/Searcher/Embedder/Refresher/Traverser/StatusReader.

- [x] Зафиксировать фактические требования к владению и перемещению между потоками
  у wire/open constructors и тестов. Не добавлять Send, Arc или Mutex механически.
  Если выявится обязательный конфликт Send с выбранным trait object, показать его
  отдельным решением перед изменением требуемого контракта.
- [x] В owned services заменить только параметр S на Box<dyn Storage>; у status —
  Box<dyn StatusStorage>. В существующих borrowed местах сохранить заимствование.
  Использовать dyn GraphDiagnostics там, где он убирает оставшийся G без дополнительных
  обёрток; lifetime у borrowed dependencies остаётся явным.
- [x] Обновить реальные constructor calls в lib.rs и test fixtures. Сохранить
  жизненный цикл зависимостей, доступность тестового состояния и lock scopes.
- [x] Остальные параметры типов проверять по реальной необходимости; не обещать
  механическое удаление всех generics и не создавать объединяющие traits ради букв.
- [x] Выполнить cargo check --locked --offline --all-targets; затем focused suites
  search_consistency, cancellation_concurrency, check_lifecycle и существующие
  unit tests service::search/service::traversal/service::document_refresher.
  Ожидание: PASS, ранние ошибки освобождают все scoped handles.

### Task 4: Проверка стоимости и окончательная приёмка

**Files:** текущие docs/superpowers/specs и этот plan; существующие storage/CLI
acceptance tests. Новые evidence directories под .artifacts/, без перезаписи receipts.

- [x] Взять baseline из первого шага Task 1 и повторить после изменений те же
  release-сценарии с теми же inputs, числом итераций, прогревом и минимум пятью
  повторами; сохранить raw timings и p50/p95. Не объявлять ускорение без измерения.
- [ ] **Частично выполнено:** показать дополнительную стоимость Box/dynamic dispatch и проверить существующие
  deadline/resource acceptance gates. Новые численные SLO не придумывать. Обнаруженное
  существенное ухудшение представить пользователю до признания изменения принятым.
- [x] Выполнить make fmt, git diff --check, затем host runner в новом evidence path:
  /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence <new directory>.
  Он включает make verify: отдельный полный дубликат не запускать.
- [x] Проверить отсутствие storage GAT и projection aliases <S as Storage>::Read/...
  в активном коде. Проверить сигнатуры, visibility, mock/wrapper implementations,
  import direction и lib.rs composition. Все traits остаются в service/mod.rs.
- [x] Выполнить независимое review финального diff; исправить важные findings,
  повторив необходимые проверки изменённого дерева. Проверить затронутые Markdown links.
- [x] Обновить согласованные spec/AGENTS/architecture/decisions по фактическому решению,
  сохранив исторические snapshots. Представить команды, receipts, измерения, открытые
  вопросы и limits. Linux отмечать unrun, если отдельно не запускался.

## Согласование

Решение B и план приняты пользователем; выполнение начато. Дополнительного
разрешения на согласованные изменения не требуется. Публикация/коммиты отдельно.

## Самопроверка плана

Типы возвращаемых значений согласованы между этапами; read-этап не требует
object-safe Storage до удаления write GAT на этапе 2. Baseline снимается до
первого изменения API. Lifetime, missing status, ConfigChanged, rollback/poison,
ранние выходы и фактические thread-сценарии имеют проверку в соответствующем
этапе. Объединяющий ReadTransaction не меняет узкий status-read контракт.
Решение и план согласованы.

## Решения при выполнении

- Read/write API и общие wrappers изменены атомарно, чтобы не добавлять временные
  forwarding layers. Все профильные проверки этапов 1/2 выполнены до изменения сервисов.
- У EmbeddingService сохранён S: семь существующих сценариев передают готовый сервис
  между потоками, а Box<dyn Storage> теряет условный Send. Сохранение одного реального
  generic поддерживает прежний wire_embedder и его lifetime/auto-trait свойства без
  глобального ограничения Storage: Send и без переписывания тестов для сокрытия проблемы.
- Остальные сокращения: CheckService<D,P>, SearchService<D,E,F,M,P>,
  DocumentRefreshService<G,F,M,P>, TraversalService<F>, StatusService<'a>.
- Новые smoke/lifetime tests и все применённые семантические мутации проверены;
  временные изменения восстановлены побайтно. 204 service unit tests прошли,
  профильные storage/service suites прошли; all-target check и Clippy зелёные.
- Парный ABBA микрозамер восстановленной исходной и новой версий (одинаковое тело
  benchmark, минимум 10 замеров по 500 операций на вариант) показал p50 чтения
  4,253 → 6,975 мкс/операцию (+2,722 мкс). Cache drop: 5,154 → 6,130 мкс;
  cache commit/vector сценарии колебались. Это цена/ограничение, а не доказательство
  нейтральной производительности; пользователю результат сообщён. Это маленькие
  synthetic transactions, не benchmark всей CLI и не resource acceptance TK-16.


Дополнительные уточнения при приёмке:

- Формулировка первоначального Review Focus про отсутствие любых read views через
  HTTP была слишком широкой: согласованный search уже удерживает pinned read во время
  query inference. Сохранён исходный scope; write transaction/embed lock через HTTP
  не добавлены. Это проверяют existing search consistency и early-release сценарии.
- Две новые boxed-проверки усилены до буквального metadata ID `a.md`. Компилируемая
  мутация decoder, возвращающая `wrong.md`, ломает обе проверки; файл восстановлен.
- Случайная чистка импортов в историческом TK-30 service_probe отменена побайтно.
  Попытка компиляции его копии выявила уже существующие устаревшие read_configuration,
  read_failure/read_vector; эти API-имена не менялись данной работой. Старый probe
  сохраняет свой исторический контекст; TK-30/historical proof suites не заявляются
  как перепроверенные на текущем API. Временный example удалён.
- В текущем плане выполняются root debug/release, strict Rustdoc, canonical schema
  parity, runner tests, provider real-60s deadline, native capabilities и copied
  standalone/linkage через host runner. Linux и target-scale TK-16 RAM/provider
  assessment остаются unrun; микробенчмарк их не заменяет.


## Финальная проверка

- `/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/storage-type-simplification/host-20261101-1`: exit 0, manifest PASS; все 16 команд exit 0.
- `make verify V=1` внутри runner: all-target check, Clippy -D warnings,
  root debug/release и strict Rustdoc прошли. Каждый профиль: 72 root executables,
  **1013 passed, 0 failed, 11 ignored**, отдельно **2 compile-fail doctests passed**.
  Вложенные helper summaries исключены из totals.
- Отдельный real provider deadline: 1 passed, 60.00 seconds.
- Schema parity, runner tests, native SQLite/vec1, copied standalone executable,
  exact vector bytes/reopen/FTS/revision=1 и dynamic linkage: PASS.
- Все **452 входных SHA-256 hashes** финального host receipt совпадают с workspace.
- Новые smoke scenarios прошли. **Пять runtime mutations** (revision, metadata ID,
  writer drop, identity guard, missing status) и **два lifetime positive controls**
  выявлены тестами; временный исходный код восстановлен побайтно.
- Fresh review: ошибок transaction ownership/lifetimes/commit/drop не найдено;
  literal-ID acceptance gap исправлен и mutation-проверен. Случайная правка
  исторического oracle отменена; его исходные bytes совпадают с baseline.
- Нет изменений SQL schemas, зависимостей или CLI/JSON формата. Все application
  interfaces остаются в service/mod.rs. Storage GAT и проекции Self::Read/CacheWrite/
  VectorWrite отсутствуют в активных interfaces/implementations.
- Measurement evidence: `.artifacts/storage-type-simplification/measurements-20261101`.
  Показанная стоимость чтения +2.722us — ограничение результата, не neutral-performance
  claim. Никакого threshold/SLO сверх существующего контракта не придумывалось.
- **Незакрытая часть широкого resource-пункта:** target-scale TK-16 RAM/provider
  acceptance не выполнялась. Host deadline/native gates пройдены; Linux и отдельные
  historical proof/TK-30 suites не запускались. Нельзя считать их прошедшими по этому
  receipt или по synthetic microbenchmark. Этот пункт оставлен частично выполненным.
- Изменения остаются без коммита на issue-69, HEAD c3de57b; merge/push не выполнялись.
