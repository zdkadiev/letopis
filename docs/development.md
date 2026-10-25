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

Выборочные исключения допускаются с объяснением через `reason`; общее отключение группы или правила ради зелёной проверки недопустимо. Проектные соглашения об именах методов, константах, границах модулей и Arrange / Act / Assert проверяются на ревью; эти 17 правил сами по себе их не обеспечивают.

Официальные источники: [каталог правил Clippy 1.95.0](https://clippy.rust-lang.org/rust-1.95.0/index.html), [рекомендация выбирать отдельные pedantic lints](https://doc.rust-lang.org/clippy/lints.html#pedantic).

## Импорты Rust

Импортировать используемые типы через `use`, а не повторять полный `crate::...` путь в выражениях и сигнатурах. Варианты enum писать через импортированный тип: `SnapshotErrorKind::InternalError`. В реализации предпочитать явные импорты вместо wildcard, если нет конкретной причины для исключения.

## Размер функций и структура тестов

Функции, методы и тестовые helpers: ориентир около 40 строк, верхняя граница 60. Выделять осмысленные этапы в именованные private helpers; не сжимать форматирование и не добавлять обёртки ради счётчика. Clippy `too_many_lines` с порогом 60 считает строки кода без пустых строк/комментариев; визуальная компактность остаётся предметом ревью.

Тесты оформлять по Arrange / Act / Assert: подготовка данных, вызов проверяемого поведения, проверка результата. Для последовательных сценариев повторять этапы; проверку корректности fixture явно отличать от проверяемого поведения. Варианты таблицы проходят те же этапы внутри цикла. Сохранять независимость тестов, изоляцию изменяемого состояния и existing failure-path assertions.

Значимые повторяющиеся значения протокола/предметной области выносить в именованные константы SCREAMING_SNAKE_CASE рядом с владельцем. Например, строковые Git object kinds, object modes, длины OID и snapshot ref prefix. Не заменять каждый литерал механически и не создавать общий набор несвязанных constants.

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
