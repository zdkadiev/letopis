# TK-28 — status diagnostics final acceptance

2026-10-27. Ветка `tk-28` (base `main` `b20543e`), проверенная ревизия `faaecda`,
платформа macOS/Darwin arm64. Задача закрывает два оставшихся критерия приёмки
[карточки TK-28](../../roadmap/epics/EP-3-maintenance/tasks/TK-28-diagnostics.md)
по [спецификации](../../../../superpowers/specs/2026-10-26-tk-28-status-acceptance-design.md)
и [плану](../../../../superpowers/plans/2026-10-27-tk-28-status-acceptance.md).
Продуктовый Rust-код не изменён: все новые сценарии — тесты; единственное
исправление вне новых тестов — форматирование ранее закоммиченной assertions-строки
(`faaecda`, `cargo fmt`), не меняющее поведения.

## Критерии → выполненные сценарии

| Критерий | Тест (scope) | Команда | Ожидание | Факт |
| --- | --- | --- | --- | --- |
| storage failure не выдаётся за сохранённую ошибку | `service::document_refresher::tests::process::uncommitted_file_diagnostic_is_absent_from_status_until_republished` (`2d14922`); production-process composition в реальном child-процессе тестового бинарника, не полный product CLI | `cargo test --locked --offline --lib service::document_refresher::tests::process::uncommitted_file_diagnostic_is_absent_from_status_until_republished -- --nocapture` | PASS | **1 passed / 0 failed** (311 filtered, 1.36s) |
| Ошибка query embedding — одна строка, видна в status, без предупреждения в search, сброс успешным запросом; drain launch failure — transient warning, не сохраняется; FTS-only остаётся успешной | `tests/search_filter_fallback.rs::query_failure_is_visible_in_status_until_a_successful_query` (`84c9d03`); реальный product CLI binary, каждая операция — отдельный процесс | `cargo test --locked --offline --test search_filter_fallback query_failure_is_visible_in_status_until_a_successful_query` | PASS | **1 passed / 0 failed** (3 filtered, 9.06s) |
| — drain warning | `tests/status_command.rs::drain_start_failure_warns_without_persisting_and_keeps_fts` (`277528b`, fmt `faaecda`); реальный product CLI binary | `cargo test --locked --offline --test status_command drain_start_failure_warns_without_persisting_and_keeps_fts` | PASS | **1 passed / 0 failed** (7 filtered, 1.11s) |

### Наблюдаемое поведение

- **Query failure.** Известное состояние с векторами и неизменным валидным config;
  две последовательные ошибки query embedding (HTTP 500) оставляют FTS-результаты
  (`a.md`, `b.md`) без `warnings` в ответе search; `status` в отдельном процессе
  показывает ровно один `query_embedding_failed` (второй сбой заменяет запись, не
  накапливает историю); успешный третий запрос возвращает semantic-результат
  (`semantic.md`) и очищает запись; `status` сам не вызывает provider и не чистит
  запись. Канарные строки провайдера (`PRIVATE`, `fixture-secret`) не появляются
  в stdout/stderr search и status.
- **Drain launch failure.** Реальный product `search` при наличии backlog и
  недоступном embed lock (симлинк на внешний canary-файл): ответ успешен,
  FTS находит `known.md`, ровно одно предупреждение `internal_error` /
  «cannot start background embedding drain» с пустыми `details`; provider не
  вызван (`TcpListener` без соединений); canary не утёк; после снятия fault
  свежий `status` не содержит проблем — предупреждение не сохранено.
- **Storage failure.** Валидная публикация (revision 1), затем authored-файл
  становится invalid; детерминированный поздний storage-отказ (private
  `fail_late` checkpoint в реальном child-процессе после staging проекции
  `files.error`): refresh возвращает infrastructure-ошибку (`StorageError` как
  source), Debug ошибки не содержит canary; старая публикация остаётся
  (exact old state, revision 1), свежий `status` через `StatusService` +
  `SqliteStatusStorage` (та же композиция, что в `lib.rs::run_status`) не видит
  несохранённый file diagnostic. После снятия fault повторная публикация
  сохраняет реальный безопасный diagnostic (`i.md` / `invalid_metadata`,
  revision 2), он переживает свежий status; исправление файла и новая публикация
  (revision 3) убирают проблему.

### Executed scope

Сценарии query/drain выполняют реальный собранный product CLI binary в отдельных
процессах (restart между операциями реален). Сценарий storage failure выполняет
production-composition `DocumentRefresher` + `StatusService` в реальном отдельном
child-процессе тестового бинарника с настоящими Git/SQLite/Fixtures, но не полный
product CLI binary; это заявленный более узкий scope, эквивалентный существующим
TK-10/TK-26 process-тестам, и он не выдаётся за product-binary end-to-end.

## Mutation controls

| Мутация | Ожидание | Факт |
| --- | --- | --- |
| `src/service/status.rs`: инверсия условия проекции `read_query_error` (`.is_some()` → `.is_none()`) | FAIL | FAILED на `tests/search_filter_fallback.rs:176` (problems assertion); байты восстановлены, PASS |
| `src/service/background.rs`: `Err(_) => return Ok(None)` вместо warning в lock-probe ветви | FAIL | FAILED на `tests/status_command.rs:315` (warnings assertion, `Null` ≠ массив); восстановлено, PASS |
| `src/storage/cache_write.rs`: отключение `failed`-guard в `commit` (плановая мутация) | FAIL | **PASS** — плановое ожидание неверно: ошибку оператора уже возвращает `record`, транзакция откатывается без guard; guard — defense-in-depth |
| `src/storage/cache_write.rs`: проглатывание ошибки `upsert_file` при рабочем guard | PASS (guard отклоняет commit) | **PASS** — guard единолочно предотвращает публикацию при проглоченной ошибке |
| Обе предыдущие одновременно | FAIL | FAILED: child-процесс падает на собственном утверждении «refresh обязан вернуть ошибку» (committed-несмотря-на-отказ детектирован); восстановлено, PASS |

Все мутации выполнялись в рабочем дереве и восстановлены до точных байтов
(`git diff` по продуктовым файлам пуст после каждой мутации).

## Повторно использованные evidence

- TK-26 (corruption/fail-closed, invalid-file recovery): отчёт принят в `8bc4fca`;
  captured per-profile 750 passed / 0 failed / 5 ignored. Сценарии повреждённых
  DB, current-invalid-file и reindex-recovery не перезапускались и не
  переклеймились.
- TK-27 (embedding lifecycle, bounded retries): финальный source `14d1e47`,
  итоговое ревью принято на `02b9736`; captured per-profile 785 passed / 0 failed /
  5 ignored. Query-error persistence/retry-семантика переиспользована.

## Команды и платформа

| Команда | Результат |
| --- | --- |
| `make check` | `ok` (fmt, all-target cargo check, Clippy `-D warnings`, root tests) |
| `make verify` | `ok` (make check + strict Rustdoc + release root tests) |
| `cargo test --locked --offline` (root, dev profile) | 796 passed / 0 failed / 5 ignored, 70 suites, 179.60s |
| `cargo fmt --check` | чисто после `faaecda` (исправлено нарушение из `277528b`) |
| `python3 -B tests/storage/run.py` | **не запускался**: storage schema, native stack, build и vendored inputs не менялись — только тесты и форматирование |

Платформа: macOS/Darwin arm64 (host). Linux/x86_64 для TK-28 не перезапускался.

## Границы и ограничения

TK-29 владеет handled cancellation; G-2 закрывается в TK-29, не здесь. x86_64,
физический power-loss (TK-38), resource/minimum-hardware (TK-36/G-1/TK-17) и
качество реальной модели/provider не проверялись этой работой. Успешный `status` —
point-in-time view закоммиченных данных, не гарантия неизменности живых файлов
после наблюдения. Тест storage-failure использует production-composition, а не
полный product binary (заявлено выше).