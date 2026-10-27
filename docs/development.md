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
cargo test --locked --offline
```

Для offline-проверок зависимости должны быть заранее доступны: при первом запуске
подготовить их через `cargo fetch --locked`. Проверки не загружают зависимости и не
исправляют файлы. Форматирование вручную: `make fmt`.

Перед завершением задачи, меняющей `src/`, публичную документацию или зависимости,
обязателен `make verify`: `make check`, строгий Rustdoc с `RUSTDOCFLAGS='-D warnings'`
и `cargo test --locked --offline --release`. Debug и release totals записываются
раздельно; nested helper invocations не прибавляются к корневым тестам.

## Native stack

После изменений storage/native/build/vendor дополнительно выполнить
`python3 -B tests/storage/run.py --platform host|linux --evidence <new-directory>`.
Runner проверяет schema parity, свой lifecycle, bundled SQLite/vec1 и копию
самостоятельного executable с проверкой linkage; каждый evidence path новый.
Исторические harnesses с отдельными manifest не входят в корневой gate автоматически.

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

Область команды — root crate и его tests; исторические изолированные proof harnesses с собственными Cargo.toml запускаются своими runners. Новая lint-политика не означает повторного выполнения старых TK evidence.

Источники: [группы правил Clippy](https://doc.rust-lang.org/clippy/lints.html), [настройка Clippy](https://doc.rust-lang.org/clippy/configuration.html), [Cargo lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section).

## Выбранные pedantic-проверки

В `Cargo.toml` явно включены 17 дополнительных правил из `pedantic`. Это выбор для текущего проекта и обучения Rust; весь набор `pedantic` не включается. Все имена и принадлежность к группе сверены с закреплённым Clippy 1.95.0. Уровень каждого правила — `warn`; текущая проверка качества не допускает предупреждений.

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
| `manual_let_else` | Использовать `let … else` для раннего выхода. |
| `match_same_arms` | Объединять одинаковые ветки `match`. |
| `redundant_closure_for_method_calls` | Передавать метод напрямую вместо лишнего замыкания. |
| `semicolon_if_nothing_returned` | Явно завершать действие, возвращающее `()`. |
| `unnested_or_patterns` | Объединять альтернативы внутри общего шаблона. |
| `doc_markdown` | Оформлять идентификаторы в документации как код. |
| `missing_errors_doc` | Описывать ошибки публичного fallible API в `# Errors`. |
| `missing_panics_doc` | Описывать возможные паники публичного API в `# Panics`. |
| `return_self_not_must_use` | Помечать подходящие методы, возвращающие `Self`, атрибутом `#[must_use]`. |

Ранее включённый `too_many_lines` также относится к `pedantic` и сохраняет лимит 60 строк кода. Итого выбрано 18 правил этой группы.

Практический пример безопасного сужения:

```rust
use std::num::TryFromIntError;

fn convert_count(value: u64) -> Result<u8, TryFromIntError> {
    u8::try_from(value)
}
```

Вместо подавления `missing_errors_doc` описывать конкретные ошибки: например, `ByteSpan::new` возвращает `ValueError::InvalidRange`, если начало больше конца. `# Panics` добавляется при реальной возможности паники; пустые разделы не нужны. Подсказку о заимствовании или преобразовании проверять по смыслу: lint помогает ревью, но не заменяет понимание контракта.

Выборочные исключения допускаются с объяснением через `reason`; общее отключение группы или правила ради зелёной проверки недопустимо. Проектные соглашения об именах методов, константах, границах модулей и Given / When / Then проверяются на ревью; эти 17 правил сами по себе их не обеспечивают.

Официальные источники: [каталог правил Clippy 1.95.0](https://clippy.rust-lang.org/rust-1.95.0/index.html), [рекомендация выбирать отдельные pedantic lints](https://doc.rust-lang.org/clippy/lints.html#pedantic).

## Импорты Rust

Импортировать используемые типы через `use`, а не повторять полный `crate::...` путь в выражениях и сигнатурах. Варианты enum писать через импортированный тип: `SnapshotErrorKind::InternalError`. В реализации предпочитать явные импорты вместо wildcard, если нет конкретной причины для исключения.

## Размер функций и структура тестов

Функции, методы и тестовые helpers: ориентир около 40 строк, верхняя граница 60. Выделять осмысленные этапы в именованные private helpers; не сжимать форматирование и не добавлять обёртки ради счётчика. Clippy `too_many_lines` с порогом 60 считает строки кода без пустых строк/комментариев; визуальная компактность остаётся предметом ревью.

Все unit- и integration-тесты оформлять по Given / When / Then: подготовка данных, вызов проверяемого поведения, проверка результата. Этапы отмечать комментариями `// Given`, `// When`, `// Then` в Rust и `# Given`, `# When`, `# Then` в Python; если вызов находится прямо в проверке, использовать `When / Then`. Для последовательных сценариев повторять этапы; проверку корректности fixture явно отличать от проверяемого поведения. Варианты таблицы проходят те же этапы внутри цикла. Сохранять независимость тестов, изоляцию изменяемого состояния и existing failure-path assertions.

Значимые повторяющиеся значения протокола/предметной области выносить в именованные константы SCREAMING_SNAKE_CASE рядом с владельцем. Например, строковые Git object kinds, object modes, длины OID и snapshot ref prefix. Не заменять каждый литерал механически и не создавать общий набор несвязанных constants.

## Моки интерфейсов

Для unit-тестов интерфейсов приложения используется закреплённый dev-dependency `mockall`. Добавляйте `#[cfg_attr(test, mockall::automock)]` только к нужному trait и реэкспортируйте mock через `service/mod.rs` под `#[cfg(test)]`, если его используют другие модули. В тесте задавайте возвращаемые значения и существенные ожидания по аргументам и числу вызовов; `mockall::Sequence` проверяет порядок между интерфейсами. Продуктовая сборка не содержит mock-типы.

В integration tests библиотека собирается без `cfg(test)`, поэтому для внешнего trait можно объявить локальный mock через `mockall::mock!`, как в `tests/cli_session_failure.rs`. Этот же macro с явно именованными lifetime поддерживает `Embedder` и `EmbeddingProvider`, которые `automock` не генерирует из-за заимствованных параметров. Поскольку ручное объявление повторяет сигнатуры, используйте его только при реальной пользе. Для моделей состояния, файловых блокировок, транзакций и реальных процессов сохраняйте fakes и интеграционные fixtures; не меняйте production-интерфейс только ради mock. Scoped storage interfaces с generic associated lifetimes оставлены реальному SQLite и специализированным wrappers.

## Имена функций и методов

Имена пользовательских функций и методов должны быть глаголами или глагольными формами, описывающими действие: `read_entries`, `build_command`, `verify_object`, `get_kind`. Для собственных accessors использовать `get_`, для предикатов — `is_` / `has_` или другой ясный глагол. Сохранять общепринятые Rust-формы конструкторов и преобразований (`new`, `new_with_cause`, `as_str`, `to_hex`, `into_inner`) и имена, заданные реализуемым trait (`fmt`, `source`, `drop`). Правило действует и для тестовых helpers; имена самих тестов описывают проверяемое поведение.


## Модули и файлы реализаций

Следовать [архитектурной структуре](zkit/letopis/architecture.md#program-structure) и [D-99](zkit/letopis/decisions.md#D-99). Имена описывают ответственность/технологию: Storage, SqliteStorage, GitSnapshots, GitCli; суффиксы Port/Adapter и каталоги ports/adapters/contracts не нужны.

Небольшие интерфейсы приложения объявлять в service/mod.rs. mod.rs также объявляет дочерние модули и явно реэкспортирует необходимые имена. Каждую конкретную реализацию держать в отдельном файле вместе со структурой и её inherent/trait impl: git/cli.rs, storage/sqlite.rs, отдельный файл для каждого реализованного сервиса. Реализации не складывать в mod.rs. При росте интерфейсов выделять приватные файлы по ответственности и сохранять короткий публичный путь через реэкспорт. Будущие пустые компоненты не создавать.

Зависимости направлены к domain и принадлежащим service интерфейсам; конкретные реализации связывает lib.rs. Документация описывает целевую раскладку D-99, а перенос существующего кода выполняется по плану TK-9.


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

Для одноразового обновления:

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
