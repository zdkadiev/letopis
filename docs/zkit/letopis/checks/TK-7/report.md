# TK-7 — доменные значения и Markdown X-4

Дата: 2026-10-21. Статус: done в локальной ветке TK-7; финальные проверки и исправления ревью завершены. Ветка `TK-7`, исходный `main@48b8ed8`. Слияния, push и изменения GitHub issue не выполнялись.

## Результат

Создан корневой library crate `letopis`: чистые доменные значения в `src/domain/`, read-only адаптер в `src/adapters/markdown/`. Вызов `parse(&RepoPath, &[u8])` возвращает `Result<Option<ParsedFile>, Vec<Diagnostic>>`; None означает неразмеченный документ, Err — невалидный файл без частичной проекции. ParsedElement содержит доменный Element и временные search_text/search_hash. Файлы, источники и БД функция не читает и не пишет.

Файл-элементы и разделы адресуются путём/якорем, метаданные created_at/source_snapshot валидируются строго. Элементы получают точные byte/line ranges и SHA-256 исходных байтов. Только inline links с letopis title создают рёбра-тройки; includes определяет metavertex, external адресует локальный source. Дедупликация, self-loop, root-relative нормализация и диагностики едины для потребителей.

Единственный renderer работает по events всего файла: сохраняет подписи ссылок, собственный текст и code examples, исключает metadata/назначения/heading attrs. Поэтому служебные изменения не пересчитывают поисковый hash. Ни CLI, ни сериализатора, ни async/runtime/SQLite/Git/provider adapter в этой задаче нет.

## Приёмка

| Критерий TK-7 | Проверки | Фактический результат |
|---|---|---|
| X-4 элементы, ID, ссылки/source URI, derived kind | domain_values; markdown_elements; markdown_links | macOS PASS; Linux PASS |
| Точные bytes/SHA-256/created_at | golden_elements_have_exact_bytes_and_metadata; unicode_crlf_and_eof_have_exact_spans; snapshot_metadata_preserves_full_oid_and_zero_created_at; markdown_search | macOS PASS; Linux PASS |
| Domain отделён от адаптеров | зависимости/импорты src/domain и cargo tree; чистый input/output parse без I/O | проверено по исходникам; нет pulldown/serde/SQL/HTTP/Git imports в domain |

30 тестов: 6 domain + 10 elements + 6 links + 8 search. macOS ARM64: cargo test --locked, cargo fmt --all --check, cargo clippy --locked --all-targets -- -D warnings PASS; [полный лог](macos-arm64.log). Linux glibc ARM64: те же 30 tests, fmt/clippy PASS в `docker run --rm --network none`; [лог](linux-arm64.log). Подготовка dependencies выполнялась с сетью, сам прогон — без неё. Точные source/log SHA-256, версии и argv/image ID — [verification.json](verification.json).

## Доказательства TDD и найденный случай

Наблюдались компилируемые RED: шесть domain tests, девять element tests, четыре link tests и семь search tests; затем GREEN. [RED/GREEN logs](tdd/). Независимые byte/line/hash эталоны записаны в tests/fixtures/markdown/expected.json; oracle создан Python hashlib по заданным вручную границам и строкам, Rust extractor не участвует в получении ожиданий.

CRLF дал отдельный дефект: pulldown выдаёт newline служебной строки отдельным Html event, поэтому фильтр только текста комментария оставлял лишний перенос в search text. `crlf_metadata_line_does_not_add_search_whitespace` воспроизведён RED→GREEN. Теперь renderer исключает весь валидированный byte range metadata, а не ищет маркеры глобальной заменой. Сами исходные CRLF bytes не меняются. Git атрибуты fixture отключают EOL conversion и объявляют cr-at-eol для whitespace check.

## Воспроизводимость

С macOS/Linux source tree: `cargo test --locked`; `cargo fmt --all --check`; `cargo clippy --locked --all-targets -- -D warnings`. Rust 1.95.0, exact dependencies и lockfile. Для Linux: `docker build -f tests/markdown/Dockerfile -t letopis-tk7-check .`, затем `docker run --rm --network none letopis-tk7-check`. Dockerfile включает только src и нужные integration tests/fixtures; .dockerignore ограничивает передаваемый context.

## Границы доказательства и передача

- Нет полной проверки существования targets, symlink containment или фактических строк source: parser делает лексические проверки, реальные FS/Git проверки принадлежат адаптерам следующих задач.
- Не проверяются межфайловые includes cycles, текущая ссылочная целостность и source.created_at по всем ссылающимся знаниям — TK-18 и последующие задачи проекции/источников.
- Сохранение последней валидной SQL-проекции — TK-10; runtime/CLI — TK-11; embeddings — TK-13. Не подтверждены RAM/latency/качество retrieval или x86_64.
- Предыдущие proof harnesses не переписаны под новую grammar; они остаются самостоятельными ограниченными доказательствами. В начале ветки свежие 8 transport Rust tests PASS, полные TK1…TK6 повторно не заявляются.
- Авторинг знаний агентом остаётся вне scope. Тело issue #7 содержит историческое parser/serializer; актуальные docs и эта реализация — parser-only.

## Финальное ревью

Один свежий reviewer gpt-6-astra/high проверил всю ветку: 2 Important, 0 Critical/Minor. Оба Important подтверждены локальными RED и исправлены за один проход: metadata внутри raw HTML больше не объявляет элемент; ссылка в image alt не создаёт ребро, а ссылка вокруг Image сохраняется. [Исходные findings, решения и RED→GREEN](review.md). После исправлений полный suite **30/30**, fmt/clippy PASS на macOS и Linux ARM64. Повторного ревью не было; исправления доказаны regression tests и полными прогонами. Отложенных minor нет.

Исторические receipts до исправлений (27 tests) сохранены в [before-review-fixes/](before-review-fixes/). Текущий verification.json относится к исправленному коду. [Журнал исполнения](execution-ledger.md) содержит все решения Native и переходы RED→GREEN; технический summary task-done с «0 tests» — последний пустой doctest target, а не суммарное число integration tests.

Код исправлений: `3aff95f`; текущие SHA-256 исходников/fixtures и log receipts закреплены в verification.json. Ветка сохраняется для интеграции по решению пользователя; main/GitHub issue не изменялись. Расписание: 45 tasks, 333 expected hours, 34 weeks; оценки/зависимости сохранены.
