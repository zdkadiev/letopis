# Rust: проверки качества

Для продуктового crate и root tests обязательны rustfmt, диагностика rustc и Clippy. Rust 1.95.0, rustfmt и Clippy закреплены в `rust-toolchain.toml`; стандартный формат rustfmt сохраняется без отдельного файла настроек.

Из корня запустить `make check`, из другого каталога — `make -C <checkout> check`.
Makefile — единственный источник команд; исторический `scripts/check-rust.sh`
сохранён и делегирует этому target. `make check` и `make verify` печатают одну
строку `ok` при успехе и полный вывод упавшего шага при ошибке. Для полного
лога каждого шага используйте `V=1`, например `make verify V=1`; platform runners
передают этот флаг явно, чтобы evidence сохраняло summaries тестов.

`make check` выполняет:

```sh
cargo fmt --all --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline --example fixed_value_lint
cargo run --locked --offline --example fixed_value_lint
cargo test --locked --offline
```

Проверка `fixed_value_lint` ищет в `src`, `tests` и `examples` простые функции
без аргументов, которые только создают фиксированную ошибку или литерал
`CommandResponse`. Это синтаксическая проверка, а не анализ типов или потока
данных: она не раскрывает макросы и не охватывает произвольные типы ответов.
Обобщённые функции и `impl`, конструкторы `new` и реализации traits исключены.
Саму проверку и её
тесты выполняет `make check`; зависимость `syn` используется только при разработке.

Для offline-проверок зависимости должны быть заранее доступны: при первом запуске
подготовить их через `cargo fetch --locked`. Проверки не загружают зависимости и не
исправляют файлы. Форматирование вручную: `make fmt`.

Перед завершением задачи, меняющей Rust-код, Rust API documentation или зависимости,
обязателен `make verify`: `make check`, строгий Rustdoc с `RUSTDOCFLAGS='-D warnings'`
и `cargo test --locked --offline --release`. Debug и release totals записываются
раздельно; nested helper invocations не прибавляются к корневым тестам.
Markdown-only изменения требуют проверки затронутых ссылок и согласованности текста.

## Native stack

После изменений storage/native/build/vendor дополнительно выполнить
`python3 -B tests/storage/run.py --platform host|linux --evidence <new-directory>`.
Runner проверяет schema parity, свой lifecycle, bundled SQLite/vec1 и копию
самостоятельного executable с проверкой linkage; каждый evidence path новый.
Текущие acceptance runners работают с продуктовым crate. Прежние standalone-прототипы сохранены как [исторические исходники](#historical-proof-harnesses).

## Historical proof harnesses

Standalone-прототипы TK-1…TK-4/TK-6 (`tests/storage-build/`, `publication/`,
`embedding/`, `runtime/`, `transport/`) и прежний `tests/markdown/Dockerfile`
удалены из отслеживаемого дерева 2026-11-02. Они проверяли ранние реализации;
текущие продуктовые тесты, fixtures и acceptance runners сохранены.

Локальная копия исходников, незакоммиченных правок и build caches находится в
игнорируемом `.artifacts/retired-tests/2026-11-02/`; там же сохранены ранее удалённые pilot tools,
`manifest.json` с хешами и `RESTORE.txt`. Этот каталог не поставляется через Git.
Коммитные версии доступны в истории Git, в том числе в [v0.1.0](https://github.com/zdkadiev/letopis/tree/v0.1.0/tests).
Для воспроизведения старого evidence нужен именно указанный в нём source revision,
а не текущий checkout или произвольно выбранный tag. Отчёты и receipts в
`docs/zkit/letopis/checks/` сохранены без изменений.

## Обработка SIGINT

`signal-hook = "=0.3.18"` закреплён для обработки интерактивного SIGINT в
исполняемом пути `run_process_cli`. Синхронный итератор `Signals` работает в
отдельном std-потоке; при сигнале он пишет единственный `cancelled`-конверт через
общий `ResponseWriter` и завершает процесс с кодом 1. `Handle::close` в `Drop`
будит блокирующую итерацию и позволяет присоединить поток при обычном
завершении. Async runtime, InterruptHandle и второй обработчик не добавляются.

## Правила

Уровни хранятся в `Cargo.toml`:

- `[lints.rust] warnings = "deny"`: предупреждения компилятора считаются ошибками.
- `clippy::all`: стандартные группы correctness, suspicious, style, complexity и perf. Точный набор определяется закреплённой версией Clippy.
- `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented`: запрещены отладочные macro и незаконченные заглушки.
- `-D warnings`: финальная проверка не допускает предупреждений, в том числе Clippy.

Группы pedantic, nursery и restriction целиком не включены. Нужный дополнительный lint выбирается отдельно по конкретной пользе. Локальное исключение допустимо для конкретного правила с объяснением через `reason`, а не общим отключением предупреждений для crate.

`clippy.toml` предназначен для параметров отдельных правил (например, порогов сложности). Сейчас в нём задан `too-many-lines-threshold = 60`; соответствующий `too_many_lines` включён отдельно в Cargo.toml. Остальные уровни правил также задаются в Cargo.toml. Тестовые assert/unwrap не запрещаются общей группой restriction.

Область команды — root crate и его tests. Новая lint-политика не означает повторного выполнения старых TK evidence; порядок доступа к прежним прототипам описан в [исторических исходниках](#historical-proof-harnesses).

Источники: [группы правил Clippy](https://doc.rust-lang.org/clippy/lints.html), [настройка Clippy](https://doc.rust-lang.org/clippy/configuration.html), [Cargo lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section).

## Выбранные pedantic-проверки

В `Cargo.toml` явно включены 20 дополнительных правил из `pedantic`. Это выбор для текущего проекта и обучения Rust; весь набор `pedantic` не включается. Все имена и принадлежность к группе сверены с закреплённым Clippy 1.95.0. Уровень каждого правила — `warn`; текущая проверка качества не допускает предупреждений.

| Правило | Чему учит |
|---|---|
| `cast_possible_truncation` | Проверять сужающие числовые преобразования. |
| `cast_possible_wrap` | Учитывать переполнение знакового результата. |
| `cast_precision_loss` | Учитывать потерю точности числа. |
| `cast_sign_loss` | Проверять знак перед преобразованием в unsigned. |
| `checked_conversions` | Использовать `TryFrom` вместо ручной проверки границ. |
| `implicit_clone` | Явно обозначать клонирование. |
| `inefficient_to_string` | Избегать неэффективного строкового преобразования. |
| `needless_pass_by_value` | Заимствовать аргумент, когда владение не требуется. |
| `fn_params_excessive_bools` | Заменять большое число булевых параметров выразительными типами. |
| `struct_excessive_bools` | Избегать структур со многими независимыми булевыми полями без проверки модели состояния. |
| `unnecessary_wraps` | Не возвращать `Result` или `Option` без возможного альтернативного исхода. |
| `manual_let_else` | Использовать `let … else` для раннего выхода. |
| `match_same_arms` | Объединять одинаковые ветки `match`. |
| `redundant_closure_for_method_calls` | Передавать метод напрямую вместо лишнего замыкания. |
| `semicolon_if_nothing_returned` | Явно завершать действие, возвращающее `()`. |
| `unnested_or_patterns` | Объединять альтернативы внутри общего шаблона. |
| `doc_markdown` | Оформлять идентификаторы в документации как код. |
| `missing_errors_doc` | Описывать ошибки публичного fallible API в `# Errors`. |
| `missing_panics_doc` | Описывать возможные паники публичного API в `# Panics`. |
| `return_self_not_must_use` | Помечать подходящие методы, возвращающие `Self`, атрибутом `#[must_use]`. |

Ранее включённый `too_many_lines` также относится к `pedantic` и сохраняет лимит 60 строк кода. Итого выбрано 21 правило этой группы. Узкое исключение для `maybe_fail_sync` сохраняет одинаковую fallible-сигнатуру в обычной сборке и при инъекции ошибок в тестах.

Практический пример безопасного сужения:

```rust
use std::num::TryFromIntError;

fn convert_count(value: u64) -> Result<u8, TryFromIntError> {
    u8::try_from(value)
}
```

Вместо подавления `missing_errors_doc` описывать конкретные ошибки: например, `ByteSpan::new` возвращает `ValueError::InvalidRange`, если начало больше конца. `# Panics` добавляется при реальной возможности паники; пустые разделы не нужны. Подсказку о заимствовании или преобразовании проверять по смыслу: lint помогает ревью, но не заменяет понимание контракта.

Выборочные исключения допускаются с объяснением через `reason`; общее отключение группы или правила ради зелёной проверки недопустимо. Проектные соглашения об именах методов, константах, границах модулей и Given / When / Then проверяются на ревью; эти 20 правил сами по себе их не обеспечивают.

Официальные источники: [каталог правил Clippy 1.95.0](https://clippy.rust-lang.org/rust-1.95.0/index.html), [рекомендация выбирать отдельные pedantic lints](https://doc.rust-lang.org/clippy/lints.html#pedantic).

## Импорты Rust

Импортировать используемые типы через `use`, а не повторять полный `crate::...` путь в выражениях и сигнатурах. Варианты enum писать через импортированный тип: `SnapshotErrorKind::InternalError`. В реализации предпочитать явные импорты вместо wildcard, если нет конкретной причины для исключения.

## Размер функций и структура тестов

Функции, методы и тестовые helpers: ориентир около 40 строк, верхняя граница 60. Выделять осмысленные этапы в именованные private helpers; не сжимать форматирование и не добавлять обёртки ради счётчика. Clippy `too_many_lines` с порогом 60 считает строки кода без пустых строк/комментариев; визуальная компактность остаётся предметом ревью.

Все unit- и integration-тесты оформлять по Given / When / Then: подготовка данных, вызов проверяемого поведения, проверка результата. Этапы отмечать комментариями `// Given`, `// When`, `// Then` в Rust и `# Given`, `# When`, `# Then` в Python; если вызов находится прямо в проверке, использовать `When / Then`. Для последовательных сценариев повторять этапы; проверку корректности fixture явно отличать от проверяемого поведения. Варианты таблицы проходят те же этапы внутри цикла. Сохранять независимость тестов, изоляцию изменяемого состояния и existing failure-path assertions.

Значимые повторяющиеся значения протокола/предметной области выносить в именованные константы SCREAMING_SNAKE_CASE рядом с владельцем. Например, строковые Git object kinds, object modes, длины OID и snapshot ref prefix. Не заменять каждый литерал механически и не создавать общий набор несвязанных constants.

## Моки интерфейсов

Для unit-тестов интерфейсов приложения используется закреплённый dev-dependency `mockall`. Добавляйте `#[cfg_attr(test, mockall::automock)]` только к нужному trait в `service/mod.rs`; сгенерированные mock-типы доступны там же при `#[cfg(test)]`. В тесте задавайте возвращаемые значения и существенные ожидания по аргументам и числу вызовов; `mockall::Sequence` проверяет порядок между интерфейсами. Продуктовая сборка не содержит mock-типы.

В integration tests библиотека собирается без `cfg(test)`, поэтому для внешнего trait можно объявить локальный mock через `mockall::mock!`, как в `tests/cli_session_failure.rs`. Этот же macro с явно именованными lifetime поддерживает `Embedder` и `EmbeddingProvider`, которые `automock` не генерирует из-за заимствованных параметров. Поскольку ручное объявление повторяет сигнатуры, используйте его только при реальной пользе. Для моделей состояния, файловых блокировок, транзакций и реальных процессов сохраняйте fakes и интеграционные fixtures; не меняйте production-интерфейс только ради mock. `CacheDatabase` и `VectorDatabase` предоставляют closure scopes: вложенные операции одного владельца присоединяются к открытому scope, а только внешний scope завершает транзакцию. Фикстуры используют те же интерфейсы; transaction handle в сервис не передаётся.

## Имена функций и методов

Канонические соглашения об именах, порядке кода, SQL и описании фаз тестов
находятся в [AGENTS.md](../AGENTS.md#rust-conventions). Получение данных называется
`get_entries`; имена внешних и standard-library traits сохраняются, а собственные
traits приложения переименовываются вместе с реализациями. Новые и изменяемые
тесты получают полные описания фаз; нетронутые тесты остаются retrofit backlog.


## Модули и файлы реализаций

Следовать [архитектурной структуре](zkit/letopis/architecture.md#program-structure) и [D-99](zkit/letopis/decisions.md#D-99). Имена описывают ответственность/технологию: Storage, SqliteStorage, GitSnapshots, GitRepository; суффиксы Port/Adapter и каталоги ports/adapters/contracts не нужны.

Все application-owned traits объявлять непосредственно в service/mod.rs, включая Storage и интерфейсы транзакций. mod.rs также объявляет дочерние модули и явно реэкспортирует необходимые имена. Каждую конкретную реализацию держать в отдельном файле вместе со структурой и её inherent/trait impl: git/repository.rs, storage/sqlite.rs, отдельный файл для каждого реализованного сервиса. Реализации не складывать в mod.rs. Интерфейсы не выделять в отдельные приватные файлы; большой размер центрального service/mod.rs здесь допустим. Общие значения и ошибки находятся в service/types.rs и service/error.rs соответственно. Будущие пустые компоненты не создавать.

Общие публичные значения, requests/responses и pure validation собраны в
`service/types.rs`; ошибки, classifications, diagnostics, persisted failure records
и result aliases — в `service/error.rs`. Оба модуля публичны, прежние короткие
пути `service::...` сохранены явными реэкспортами. Для этих центральных файлов
допустим большой размер; правила закреплены в [AGENTS.md](../AGENTS.md).

Каталог `src/service/` плоский: реализация каждого сервиса, его приватные helper-модули и unit-тесты находятся в одном `.rs` файле. Helper-модули и тесты объявляются inline, с сохранением собственных пространств имён и visibility; production-код предшествует тестам. Для этих файлов с inline-тестами действует согласованное исключение из рекомендации по длине файла в [AGENTS.md](../AGENTS.md).

Кроме центральных `mod.rs`, `types.rs`, `error.rs` и приватного `utils.rs`, каждый файл непосредственно в `src/service/` содержит конкретную реализацию сервиса. Общие для нескольких сервисов helpers находятся в `service/utils.rs`; helpers одного сервиса остаются в его файле. Так, проверка актуальности knowledge и сбор check/status findings находятся в utils, а projection — приватный inline-модуль `DocumentRefreshService`. Отдельный сервис-обёртка для helper-функций не создаётся.

Зависимости сервисов внедряются при сборке и используются через `self.dependency`; internal helper-функции не получают и не передают их отдельными параметрами. Для операции с раздельно заимствованными collaborators допустим приватный receiver с собственными методами. Общие free helpers работают с данными, domain остаётся без инфраструктуры. Операции хранилища используют активный closure scope своего владельца, без передачи transaction handle между сервисами.

`ProvenanceService<F>` владеет Git через `Box<dyn VerifiedGitSnapshots>`: общий application-owned trait объединяет `GitSnapshots` и `SnapshotVerifier` и автоматически реализован для типов с обоими контрактами. Git type parameter удалён также из локального historical cache. Наблюдение файлов использует `FileReader`; блокировка нужна только владельцам публикации и drain и передаётся через `FileLocker` с ассоциированным RAII-ресурсом. Время и ожидание повторов предоставляет отдельный `Clock`, реализованный синхронным `SystemClock` в `process/`.

Тот же inline-подход и исключение по длине файла применяются к unit-тестам в `src/cli/response.rs` и файлах реализаций плоского `src/git/`: вложенные test-модули сохраняют свои имена и visibility, отдельные каталоги для них не создаются.

Зависимости направлены к domain и принадлежащим service интерфейсам; конкретные реализации связывает lib.rs. Документация описывает реализованную раскладку D-99; исторический план TK-9 сохраняет шаги переноса своей ревизии.


## Файлы и пути

`LocalFileSystem` реализует `FileReader`, `FileWriter` и `FileLocker` из
`service/mod.rs`. Он оперирует относительными путями, байтами, метаданными,
идентичностями и блокировками файлов; конфигурационные и database-роли ему
неизвестны. `open_filesystem` закрепляет существующий корень без создания
состояния. Сервисы выбирают `.letopis/config.json`, `index.lock`, `embed.lock`
и отображают `FileError` в собственные безопасные ошибки. Только документные
потребители исключают стабильный `NonRegular`; `UnsafePath`, `Changed` и I/O
остаются ошибками. Status наблюдает существующие файлы без mkdir/chmod/записи PID.

Composition в `lib.rs` проверяет исходный дескриптор каталога, идентичности
`cache.db`/`vectors.db` и отсутствие алиаса до и после каждого открытия/attachment,
включая неуспешный SQLite-вызов. Идентичности отсутствующих файлов остаются
наблюдениями одной операции; байты database-файлов для этого не читаются.
Разрешение внешних путей сохраняет проверку UTF-8 до и после canonicalization.
Исторические планы с `DatabasePaths` и role-specific интерфейсами описывают свою
исходную ревизию; действующие сигнатуры находятся в
[`service/mod.rs`](../src/service/mod.rs), реализация — в
[`filesystem/local.rs`](../src/filesystem/local.rs).

## Построение SQL-запросов

Обычные SELECT/INSERT/UPDATE/DELETE собираются fluent API SeaQuery с типизированными
именами таблиц и колонок ([D-100](zkit/letopis/decisions.md#D-100)). SeaQuery остаётся
деталью `src/storage/`: сервисы и domain не импортируют его типы. Пример формы:

```rust
let query = Query::select()
    .columns([Nodes::Id, Nodes::Kind, Nodes::Uri])
    .from(Nodes::Table)
    .and_where(Expr::col(Nodes::Id).eq(id.as_str()))
    .to_owned();
let (sql, values) = build_query(&query)?;
```

Приватный `storage/query.rs` преобразует параметры в значения rusqlite с проверкой
целочисленного диапазона; исполняющий код передаёт их через `params_from_iter` в
существующую Connection/Transaction. Не применять `to_string` для SQL с
пользовательскими значениями. Builder не проверяет существование колонок или
схему реальной БД на этапе компиляции: это проверяют storage tests.

Версии rusqlite и bundled SQLite сохраняются. Публичный sea-query-rusqlite 0.8.0
требует rusqlite ^0.38 и не подключается к нашему =0.40.2; маленькое преобразование
значений не требует второго драйвера, ORM или async runtime.

DDL остаётся в cache.sql/vectors.sql. SQL для introspection, PRAGMA/ATTACH/native
probes, FTS MATCH/bm25 и vec1 search/reset-команд остаётся явным и читаемым.
Обычный CRUD метаданных вокруг FTS/vec1 всё равно использует builder. SQL fixtures
тестов можно сохранять как независимый oracle. Миграция не меняет схему,
транзакционные границы, правила ошибок и типы application-owned интерфейсов.

## TK-20: явная пересборка кэша при обновлении

Утверждено 2026-10-25: [спецификация source identity](superpowers/specs/2026-10-25-tk-20-source-identity-design.md).
Старый parser мог сохранить URL `https://Host/a` как `https:/Host/a`.
Обычный refresh пропускает неизменённые валидные Markdown-файлы, поэтому обновление
бинарника само по себе не исправляет старые адреса. Гарантии TK-20 действуют для
свежего или явно пересобранного кэша; автоматической миграции нет.

Локальный адрес отклоняется, если после нормализации `.`/`..` и слешей путь
начинается с `http:` или `https:` без учёта регистра: например `./https:notes`,
`dir/../HTTP:notes`, `./https://Host/a`, в том числе с диапазоном строк. Такой
адрес не превращается в URL: его сохранённый ID иначе менял бы классификацию
при повторном чтении. Принятый ID повторно разбирается с тем же адресом и типом.
Корневой файл с таким префиксом нужно переименовать или поместить в однозначный
каталог (`docs/https:notes`). Грамматика RepoPath/KnowledgeId и исходное написание
настоящих абсолютных HTTP/HTTPS URL не меняются. Неверная external-ссылка следует
обычной политике invalid-file: прежняя валидная проекция сохраняется, а в свежий
кэш некорректный source/edge не попадает; исправление ссылки не требует пересборки.

Историческая процедура TK-20 до реализации CLI reindex (для текущего markup
upgrade применяется отдельный порядок issue 69 ниже):

1. Остановить все Letopis-команды и detached `embed --drain`, работающие с этим
   репозиторием. Дождаться закрытия всех соединений SQLite и освобождения блокировок.
2. В выбранном Git-корне удалить только производный `.letopis/cache.db` и его
   существующие quiescent sidecars: `cache.db-wal`, `cache.db-shm`, `cache.db-journal`.
   Сохранить `.letopis/vectors.db` и его файлы, `config.json`, `index.lock`,
   `embed.lock`, Markdown и весь Git. Файлы блокировок сохраняют идентичность;
   обычный refresh вправе записать PID текущего владельца в `index.lock`.
3. Запустить существующий `letopis check` с JSON `{}` на stdin в этом репозитории
   (либо существующий library refresh). Он создаёт кэш и восстанавливает граф/FTS
   из канонических Markdown. CLI `reindex` пока не реализован.

Этот порядок описывает управляемое обновление пользователем, а не разрешает
разработчику удалять живой кэш. `tests/source_lifecycle.rs` удаляет только кэши
своих временных fixtures после закрытия всех handles. Тест upgrade сохраняет
vector `[1.0, 0.0, 0.0]`, счётчик попыток и failure sentinel через публичный Storage.

## TK-23: версия схемы кэша 2

Утверждённые [ограниченные чтения графа](superpowers/specs/2026-10-25-tk-23-traversal-design.md#storage-access-indexes-and-compatibility)
добавляют индексы `edges_outgoing(source_id,type,target_id)`,
`edges_incoming(target_id,type,source_id)` вместо `edges_target` и частичный
`nodes_source_snapshot(source_snapshot)` для непустых корней. Поэтому точная
распознаваемая схема `cache.db` имеет версию **2**; схема `vectors.db` остаётся
версии **1**. Старый кэш безопасно отклоняется до открытия векторов; автоматической
миграции, удаления или изменения данных при несовместимости нет.

Для обновления выполнить тот же явный quiescent-порядок из TK-20 выше: остановить
все команды и detached drain, закрыть соединения и освободить блокировки, удалить
только производный `cache.db` и его существующие sidecars, затем запустить
`letopis check` с `{}` на stdin. Сохранить `vectors.db` и все его sidecars,
`config.json`, идентичность `index.lock`/`embed.lock`, Markdown и Git.
Пересборка заново читает и публикует весь корпус и создаёт индексы; её стоимость
зависит от размера корпуса. После неё неизменённый refresh читает только состояния
файлов и отдельный набор объявленных корней, а соседей можно получать ограниченными
страницами. Это не обещание постоянной памяти при первоначальной/изменённой публикации.

### Проверка bounded traversal TK-23

[Отчёт TK-23](zkit/letopis/checks/TK-23/report.md) и [machine-readable receipt](zkit/letopis/checks/TK-23/verification.json) сохраняют host native PASS на `e9a9cd4a4c9d6807b0a75d79022dc8f838796e78` и новый Linux arm64 native PASS на `741930ed871e60a0987d655cf79defdff67defd3`. В обоих профилях по684passed/0failed/5ignored в48root executables. Linux также прошёл DDL parity, native SQLite/vec1, standalone/linkage и отдельный real-deadline probe. Исходный Linux FAIL после read-only Docker сохранён исторически; новый PASS находится в отдельном evidence directory и не переименовывает прежние receipts.

Ограничение относится к BFS/results плюс двум страницам соседей, не ко всему RSS команды: первоначальная/изменённая публикация, candidate/file/root sets, чтение/разбор файлов и JSON имеют собственные расходы. Task4 independent review и I1 rereview пройдены; final whole-branch review завершён: Critical/Important product findings нет, code quality approved with M1/M2 documentation corrections; M1/M2 documentation corrections approved by scoped re-review; no open code or documentation finding; последующий docs commit не меняет captured revision. Описанный выше cache-only rebuild остаётся явной quiescent-процедурой; tests используют только временные fixtures.


## Git runtime и проверка отсутствия helpers — 2026-10-31

Продукт читает/пишет локальные Git objects/index/refs через закреплённую Rust
библиотеку без исполняемого Git. Разработке и авторским workflow Git нужен для
commits, подготовки source_snapshot, чтения inspect snapshot и независимых oracles;
SHA-256 fixtures требуют поддерживающую их версию Git. Поставка runtime не требует
Git CLI, shell, фильтров или transport helpers. Provider нужен при enabled embeddings.

`tests/git_no_subprocess.rs` выполняет реальные CLI commands с poison PATH; это
само по себе проверяет только PATH lookup. Linux gate дополнительно запускает
каждый продуктовый интервал через `strace -ff -e trace=process`, включая потомков,
в контейнере `--network none`. Fixtures/oracles работают вне traced interval.
Матрица model-free; отдельный nonempty direct drain использует только собственный
loopback fake provider. См. [Git runner](../tests/git/README.md).

Итоговый native runner уже включает `make verify`, strict docs, реальный 60 s
client deadline с fake provider и copied standalone linkage: не дублируйте перед
ним неизменённый `make verify`. Свежие receipts сохраняют exact input hashes;
старые TK-8/proof receipts остаются доказательствами своих ревизий. Эти проверки
не подтверждают x86_64, power-loss durability или произвольные filesystem faults.

Differential tree tests используют строгую семантику Git 2.50.1, совпадающую с
зафиксированным host baseline. Debian Git 2.39.5 — более слабый parse-only oracle;
его принятие malformed tree не отменяет invariant продукта. Docker runners
закрепляют независимый Git 2.50.1 с проверкой SHA-256 исходного архива; локальный
запуск parity tests также требует соответствующего strict oracle. Это test-tool
contract, не runtime-зависимость или обещание parity всех исторических Git.


## Issue 69: обязательный path и явный reindex

2026-10-31, [D-106](zkit/letopis/decisions.md#D-106),
[действующий X-4](zkit/letopis/contracts/X-4-markdown.md): только
`letopis:type=TYPE;path=ADDRESS`; `path` — обязательный терминальный canonical
адрес от Git-корня, destination — относительная навигация от документа.
Title не percent-decode: пробел имени — пробел, `%20` — буквальные три символа;
destination декодируется ровно один раз. Сравнение локальных файлов лексическое,
без fragments, existence I/O или viewer validation; remote HTTP/HTTPS source
исключён. Legacy fallback, автоматического rewrite и parser-version migration нет.

1. Дождаться завершения прежних CLI/drain и установить новый бинарник. Не смешивать
   версии на одном кэше.
2. Добавить `path` с прежним намеренным canonical ID и исправить относительные
   destinations активного корпуса. Сохранить created_at и честные source bases.
3. В Git-корне передать `{}` в `letopis reindex`, затем `{}` в `letopis check`.
   Reindex обязателен: обычный refresh вправе пропустить неизменённый файл.
4. Устранить markup findings и отдельно разобрать provenance findings. Успешный
   reindex сам по себе недостаточен: last-valid проекции invalid files сохраняются.

Для этого markup upgrade не удалять cache.db, vectors.db, config, locks,
Git-снимки или Markdown; это отдельный порядок от старых schema-upgrades выше.
`LinkPathMismatch` сохраняется успешным refresh/reindex в `files.error` и виден в
`check`/`status.result.problems` как `validation_failed`, reason
`link_path_mismatch`, фиксированное безопасное сообщение
`Markdown link path does not match letopis path`; details.path — содержащий файл.
Status read-only: до публикации новой ошибки её не видно, а после правки автора
сохранённая ошибка исчезнет только после успешного refresh/reindex. SQL failure
не заменяет опубликованное состояние. Свежий кэш не публикует часть invalid файла.
Исторические спецификации/receipts сохраняют синтаксис своей ревизии.
