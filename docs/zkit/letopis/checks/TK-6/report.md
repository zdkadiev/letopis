# TK-6 — embedding transport и offline-ошибки

Статус: **PASS / done**, 2026-10-21. Одно независимое ревью завершено; обе Important находки исправлены в одном проходе с RED→GREEN и полными зелёными прогонами исправленного кода.
База: `cc82a8f1da7ff8dc6b53dc682462a09fe851f458`, ветка `TK-6`.

## Реализация

Изолированный [proof crate](../../../../../tests/transport/README.md) проверяет
исходящий порт texts → vectors по X-2/D-83/D-96. Чистая подготовка запроса и
проверка ответа отделены от блокирующего ureq 3.4.2. У клиента нет retries,
redirects, environment proxies, Tokio/reqwest или HTTP worker. TLS явно
использует rustls-platform-verifier и системное доверие.

Foreground SIGINT: поток signal-hook Signals печатает один cancelled JSON
с exit 1; обычный ответ и отмена делят stdout once-guard до конца write/flush.
Проверены ожидание headers/body, гонка ответа с сигналом и сигнал после
напечатанного результата. Drain fixture отдельно умирает по сигналу без JSON.
Полная интеграция отмены SQL/flock/git остаётся TK-29.

Схема config остаётся строгой в корне и открытой внутри parameters.
Неизвестные параметры сохраняются, но не пересылаются; обязательные поля
запроса нельзя подменить. Только HTTP 200 является успехом. Native model и
embeddings обязательны, compatible data/index/embedding обязательны,
model/object/usage не навязываются. Проверяются количество, размерность,
конечные f32 и биекция индексов.

## Проверки

| Проверка | Свидетельство |
|---|---|
| Rust contract tables — 8 тестов | Платформенные cargo test receipts |
| Реальные HTTP/TLS/process fixtures — 16 тестов | Платформенные unittest receipts |
| Защита runner от перезаписи и ложного negative PASS — 3 теста | Платформенные unittest receipts |
| Реальный default deadline 60 s; headers/body/trickle | `test_actual_default_deadline_is_60_seconds`, `test_short_deadline_covers_headers_body_and_trickle` |
| Redirect 301/302/303/307/308, proxy traps | `test_redirect_sink_receives_no_request_or_key`, `test_proxy_environment_cannot_change_http_or_https_route` |
| Различие системного CA и Mozilla roots | Linux disposable system-store before/after и WebPki mutation |
| macOS positive TLS без Keychain edits | [Отдельный public HTTPS GET](system-tls-macos-arm64/manifest.json) через тот же Agent factory |
| Диагностика без knowledge/key/body/header/query canaries | Реальные отказные endpoints и redaction mutation |
| Текущие исходники TK-1…TK-5 совпадают с committed PASS | `prior_proofs` в обоих итоговых manifests |

[macOS ARM64](macos-arm64/manifest.json), [Linux glibc ARM64](linux-arm64/manifest.json),
[точный Docker argv и image ID](linux-invocation.json). Сборочные зависимости
подготовлены до offline прогона; Linux runner исполняется с `--network none`.
Положительный macOS HTTPS probe явно отделён от offline fixtures.

На macOS системное доверие проверено публичным сертификатом; установка
корпоративного CA в Keychain не выполнялась. Различие custom system CA от
WebPki доказано в disposable Linux-контейнере. CA/серверные ключи — явно
публичные тестовые данные; хранилище доверия хоста не менялось.

## TDD и отрицательная воспроизводимость

- Pure protocol: заглушки дали 4 ожидаемых поведенческих FAIL; реализация —
  7 PASS. Дополнительный root-prefix regression поймал `//embed`, исправлен
  с RED→GREEN; итог 8 PASS.
- Реальный CLI stub дал ожидаемый FAIL для обоих API, затем реальный клиент
  прошёл проверки. Первая полная suite выявила две ошибки fixture/oracle:
  регистр имён HTTP headers и native payload в OpenAI proxy fixture.
- Runner stub дал 6 assertion failures в 3 методах; после реализации 3 PASS.
- Mutation redirects выявила слабый тест: ureq самостоятельно не повторяет
  POST body при 307/308, поэтому только 307 не отличал неверную redirect
  policy. Добавлены 301/302/303 и наблюдение GET/POST/CONNECT.
- Compiling controls: count, dimensions, indices, redirects, proxies,
  deadline, cancellation, stdout once-guard, cancelled-exit, redaction; system-trust на Linux.
  Каждая мутация должна собраться и упасть на своей именованной behavioral
  assertion; ошибка компиляции/импорта/чужой тест не считаются DETECTED.
  Все изменения выполняются в disposable source copies с проверкой
  original/mutated/restored SHA256. Реальный worktree не мутируется.

Предварительные RED/исследовательские журналы не заменяют итоговые receipts.
Они сохраняются отдельно в `tdd/`; исторические FAIL не переписываются как PASS.

## Независимое ревью

[Одно полное ревью](review.md) нашло две Important проблемы: cancelled с exit 0 при гонке и race oracle, отвергавший допустимый success/exit 1. Обе исправлены с RED→GREEN в одном проходе. Для межплатформенной воспроизводимости main удерживается после подтверждённого HTTP success перед выводом; cancelled печатается первым, затем main освобождается. Compiling cancelled-exit mutation проверяет защиту кода выхода. Повторного ревью нет. Окончательные прогоны: на каждой ОС 8 Rust + 19 Python тестов PASS; 10 macOS / 11 Linux compiling controls DETECTED. Исходники до/после и все hashes evidence совпали. Deferred minors: none.

## G-0

| Задача | Основание |
|---|---|
| TK-1 | [Сборка/SQLite/FTS5/vec1](../TK-1/report.md); статическая продуктовая сборка vec1 остаётся TK-9 |
| TK-2 | [Публикация и прерывания](../TK-2/report.md); не физический power-loss proof |
| TK-3 | [Векторные read view и stale-text](../TK-3/report.md) |
| TK-4 | [flock/drain/process death](../TK-4/report.md); новый foreground cancelled проверен TK-6, полный CLI — TK-29 |
| TK-5 | [Протокол пилота](../TK-5/report.md); сам utility pilot не запускался |
| TK-6 | [macOS PASS](macos-arm64/manifest.json), [Linux PASS](linux-arm64/manifest.json), [review и исправления](review.md) |

**G-0: continue → TK-7.** Все acceptance TK-1…TK-6 подтверждены в своих границах; [проверка прошлых задач](prior-tasks-verification.json) и десять committed manifests сверены с текущими исходниками. Это допуск к основной разработке, не готовность продукта или измеренная польза памяти.

## Границы результата

Локальные endpoints синтетические. `ollama --version` на машине: 0.40.7;
вызов модели/загрузка весов не выполнялись. Версия 0.40.6 из R-4 — ранее
проверенная, не гарантированный минимум. Нет доказательства совместимости
любого OpenAI-compatible сервера, качества модели или пользы памяти.

Проверены macOS/Linux glibc ARM64; x86_64 остаётся assumed до TK-43.
Network deadline не является CPU deadline JSON parsing или отсечкой search.
Новое ограничение размера HTTP body не вводилось; hostile memory exhaustion
остаётся вне этого узкого transport proof. Retry/storage lifecycle — TK-27,
реальная inference/offline/security проверка — TK-41.
