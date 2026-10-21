# TK-6 — embedding transport и offline-ошибки

Статус: rev3, черновик после сверки с обновлённым main и двух независимых проверок соответствия. Продуктовые решения не пересматриваются. Ветка `TK-6` перебазирована на `main@cc82a8f1da7ff8dc6b53dc682462a09fe851f458`; исходная база — `379c2f3`. Пользователь разрешил реализацию сообщением Go on; код и проверки находятся в tests/transport; итоговые macOS/Linux receipts PASS, одно ревью и исправления завершены. [Отчёт](../../zkit/letopis/checks/TK-6/report.md).

## Основание и цель

[Карточка TK-6](../../zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-6-provider-harness.md), [X-2](../../zkit/letopis/contracts/X-2-embeddings.md), [машиночитаемая схема](../../zkit/letopis/contracts/schemas/X-2-embeddings.yaml), [D-83](../../zkit/letopis/decisions.md#D-83), [D-96 — принятый стек](../../zkit/letopis/decisions.md#D-96), FR-22/FR-30/NFR-6 и соответствующие части DR-5/DR-8.

Получить воспроизводимый boundary harness двух embedding API: настоящий HTTP-клиент, управляемые локальные endpoints, наблюдаемые запросы/отказы/отмена. Необходимость настоящей модели, production executor, retries/storage, query FTS fallback и полный offline/security drill остаются у TK-13/TK-27/TK-41. Успех synthetic transport fixtures не доказывает качество embedding или соответствие любого совместимого сервера.

## Архитектурная граница — обязательна

По прямому указанию пользователя сохраняем **DDD + hexagonal, порты и адаптеры**: [architecture §4 — части и порты](../../zkit/letopis/architecture.md#4-parts), [общая архитектура](../../zkit/letopis/architecture.md), [карточка TK-7](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-7-markdown.md).

Порты определяются потребностями приложения. HTTP/Ollama/OpenAI — исходящие адаптеры одного порта `texts → vectors`; доменные инварианты не зависят от HTTP-библиотеки, JSON provider или SQLite. Markdown-кодек, Git, служебная БД и CLI остаются отдельными адаптерами; SQLite — пересоздаваемый кэш, документы/Git — источник содержимого. Модули принадлежат одному CLI-бинарнику, не отдельным сервисам.

**Обязательное ограничение, уже принятое в дизайне:** [mechanics §5](../../zkit/letopis/mechanics.md#5-timing-memory-and-concurrency-rules) прямо задаёт «Синхронный Rust I/O и std threads без async runtime». D-96 закрепляет блокирующий ureq 3, rustls-platform-verifier и обработанный SIGINT. Первая версия спецификации с Tokio противоречила существующему правилу; rev2 исправила эту ошибку. Rev3 следует новым уточнениям main, включая замену прежнего предположения об обычной смерти foreground-процесса по SIGINT.

TK-6 живёт в `tests/transport`, вне будущего production composition root. Проверка формы HTTP-запроса и декодирование ответа принадлежат транспортному адаптеру. Доменный crate, общий service locator, provider registry и новые настройки продукта ради стенда не создаются.

## Принятый контракт

- Одна конфигурация `provider/model/parameters/max_retries`; base_url задаётся явно и не отправляется в JSON. Не вводить default endpoint, provider credentials в конфигурацию или новое поле deadline.
- Все четыре поля обязательны; provider — ollama/openai, model — непустая строка, parameters — объект с base_url, max_retries — целое ≥0. Лишние корневые поля и имена credentials запрещены по X-1. Внутри parameters дополнительные поля разрешены схемой: сохранить их при разборе, но пересылать только документированные поля своего API. Не подменять открытую схему blanket-запретом неизвестных параметров.
- Ollama: POST к `<base_url>/embed`, model/input и обязательное `truncate:false`; только документированные dimensions/keep_alive/options при наличии.
- OpenAI-compatible: POST к `<base_url>/embeddings`, model/input и `encoding_format:"float"`; dimensions при наличии. Обрабатывать data по index, независимо от порядка.
- Reserved model/input/encoding_format/truncate нельзя подменить parameters: клиент строит эти поля явно и не пересылает одноимённые параметры. max_retries — не часть запроса provider. Невалидные URL без HTTP(S), с userinfo или без полного адреса отклоняются локально. При добавлении endpoint сохраняются custom path prefix и query; fragment не отправляется по HTTP. Дополнительного запрета query/fragment в X-2 нет.
- Ключ OpenAI читается из OPENAI_API_KEY процесса. Ollama не получает его автоматически. Для проверок используется только искусственный canary, реальных ключей нет.
- Успешный HTTP status — только 200 по X-2 schema. Ответ: ровно один вектор на вход; непустая согласованная ожидаемая размерность, только конечные f32; для совместимого ответа — биекция index на входы. Native требует непустую строку model и embeddings; compatible — data с index/embedding, без требования model/object/usage. Дополнительные поля ответа допустимы. Bool/null/string, duplicate/missing/out-of-range indices, malformed JSON и provider error не становятся успешными векторами.
- Одна попытка HTTP имеет внутренний deadline **60 секунд**, включая ожидание и чтение ответа. Сокращённые значения разрешены только в fixture-пути для быстрых проверок; они не меняют 60s baseline и не становятся config API. Проверить также реальное истечение default 60s на silent endpoint.
- Foreground SIGINT по [D-96](../../zkit/letopis/decisions.md#D-96) и [X-1](../../zkit/letopis/contracts/X-1-cli.md): отдельный поток signal-hook Signals печатает единственный ok:false/cancelled JSON и завершает процесс с exit 1, в том числе во время блокирующего HTTP. Вывод обычного результата и cancelled проходит через один stdout once-guard; после напечатанного результата второй JSON запрещён. SIGINT не обещает остановку remote compute. Отсоединённый drain не ставит обработчик: обычная смерть по сигналу, без JSON. Полная интеграция SQL/flock/git остаётся TK-29; управление сохранением результатов/attempts — TK-27. В HTTP-порт не добавлять CancellationToken или HTTP worker.
- Не следовать redirects, не использовать неявные environment proxies. TLS — rustls с rustls-platform-verifier и системным trust store, с проверкой certificate/hostname; WebPki defaults не заменяют выбранную конфигурацию. HTTP endpoint не получает ложной гарантии конфиденциальности.
- Ошибки форматируются локально по классу/HTTP status; не включают response body/headers, ключ, произвольный текст provider, endpoint query или исходные знания. Существующие embedding_failed/cancelled остаются границей передачи; нового product status нет.
- Для локального сценария используется явно выбранный native Ollama endpoint на loopback. Нет скрытого remote fallback. Проверка не вводит запрет явно настроенного удалённого endpoint для иных сценариев и не создаёт новое поле local mode.

## Конкретизация стенда

Стек и семантика отмены уже приняты в D-96. Организация proof crate и fixture API ниже — инженерная конкретизация для review.

- Rust 1.95.0 / edition 2024, как существующие proof crates. Изолированный crate `tests/transport`; существующие storage/runtime helpers не изменять.
- Синхронный ureq 3 с Rustls, JSON и platform-verifier; default features отключены, TLS явно выбирает системный verifier. Точная версия ureq — 3.4.2; совместимые версии всех прямых crates закрепить `=x.y.z` при первом добавлении вместе с Cargo.lock. Один блокирующий вызов, затем следующий элемент drain. Проверить отсутствие Tokio/reqwest в resolved dependency tree. TLS самостоятельно не реализовывать.
- Конкретные функции HTTP-адаптера возвращают `Result<Vec<Vec<f32>>, TransportError>` с закрытой локальной классификацией. Никакого произвольного provider error string в Display/Debug/CLI output.
- Один тестовый CLI-процесс выполняет одну синхронную попытку из JSON stdin. Foreground fixture проверяет обработанный SIGINT и once-guard D-96; отдельный drain fixture — отсутствие обработчика. Дедлайн fixture явно отделён от конфигурации продукта. Не создавать HTTP task, вспомогательный процесс для запроса или собственный scheduler; поток Signals отвечает только за сигнал/ответ/exit.
- Python 3.11+ stdlib управляет локальными HTTP/TLS fixtures, процессами и receipts. Использовать явный поддерживаемый interpreter: в основном checkout python3 ранее разрешался в 3.9.6, в worktree — в 3.12.0.
- TLS fixtures используют только тестовые сертификаты/ключи, явно помеченные несекретными. Положительную проверку системного CA выполнить в disposable Linux-контейнере: без CA — отказ, после установки CA в системное хранилище контейнера — успех тем же Client::new; hostname mismatch — отказ. Хранилище хоста/Keychain не менять. На macOS проверить выбор platform-verifier и отказ недоверенному сертификату; положительную проверку системного доверия выполнить отдельно на заранее выбранном публичном HTTPS endpoint без ключей, знаний и inference, с явной записью сетевого обращения. Такая проверка отделена от offline runner. Если она недоступна, явно оставить её непроверенной, не выдавать fixture CA injection за проверку macOS Keychain. При необходимости injection разрешён только в fixture-пути и доказывает лишь механику TLS.

## Матрица проверок

1. Точные method/path/body и canary Authorization: два API, Unicode и несколько текстов, переставленные индексы; параметры не подменяют обязательные поля.
2. Формы ответов: неправильное количество/размерность, пустые/смешанные векторы, nonfinite/overflow, ложные indices, malformed JSON, error-object на 200, отсутствие обязательных полей.
3. Реальный transport: connection refusal, non-JSON 4xx/5xx, закрытие/обрезанный ответ, silent endpoint, silent body после headers и медленный поток. Whole-attempt deadline должен прерывать все стадии ожидания.
4. Foreground SIGINT после подтверждения принятого запроса, в ожидании headers и body: ровно один cancelled JSON, exit 1 до дедлайна, без позднего успеха. Проверить гонку сигнала и ответа и отсутствие второго JSON после уже напечатанного результата. Drain fixture отдельно завершается сигналом без JSON. Тайминги fixture не являются product latency SLO.
5. Redirect sink и proxy trap: не получают ни запрос, ни canary; проверяются upper/lowercase proxy variables в изолированном child environment, без изменения окружения остальных тестов.
6. HTTPS: системный verifier, принятие системного CA и отказ недоверенному/hostname mismatch с раздельными receipts Linux/macOS; injected CA отдельно. Safe error canaries должны отсутствовать в stdout/stderr/Display/Debug, включая ответы/headers/URL credentials и input.
7. Loopback Ollama в Linux контейнере `--network none`: успех fixture; при отказе — safe failure без скрытого внешнего обращения. Это transport proof с заранее доступным endpoint, не проверка настоящей модели и её весов.
8. Отдельные компилируемые negative controls отключают проверку vectors, запрет redirects/proxies, deadline/cancel или redaction. Контроль должен упасть на своей behavioral проверке; build failure и чужая ошибка не засчитываются. Только disposable copies, исходники восстанавливать byte-for-byte.

## Артефакты и решение G-0

Сохранить исходники/lockfile, команды/версии, фактические requests с искусственными canaries, raw outputs и hashes в `docs/zkit/letopis/checks/TK-6/`. Раздельные macOS/Linux glibc receipts с архитектурой; x86_64 остаётся assumed до TK-43, ARM64 не доказывает x86_64. TDD RED/GREEN, список review findings и их dispositions. Не изменять старые receipts ради новых хэшей. Записать фактическую версию Ollama при реальном вызове; для synthetic fixtures явно указать, что Ollama не запускалась. R-4 0.40.6 — проверенная версия, не гарантированный минимум.

Перед G-0 сверить приёмку TK-1…TK-5, статусы карточек и актуальность их исходников относительно committed PASS manifests. Отрицательная воспроизводимость не считается pass. G-0 можно признать пройденным только после PASS текущего TK-6 и этой сверки; реальные resource/utility/system gates остаются впереди. Пока проверки не выполнены, G-0 открыт.

План с точными файлами, интерфейсами и RED/GREEN задачами сохраняется отдельно в `docs/superpowers/plans/`; карточка TK-6 остаётся источником статуса выполнения. Commit/merge/push и закрытие issue не входят автоматически в новый запрос на разработку.

## Проверенные первичные источники

Документация прочитана 2026-10-21, без вызовов модели и без paid API requests:

- [Ollama /api/embed](https://docs.ollama.com/api/embed): native endpoint, input/embeddings, параметры и отказ вместо усечения при truncate:false.
- [OpenAI embeddings](https://developers.openai.com/api/reference/resources/embeddings/methods/create): float encoding и data/index/embedding.
- [ureq](https://docs.rs/ureq/latest/ureq/), документация версии 3.4.2: blocking I/O и Rustls; [ConfigBuilder](https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html) — global timeout от DNS до чтения body, явный proxy(None), max_redirects(0). Это документация, не выполненные проверки реализации. Синхронный JSON parsing не становится вытесняемым; разрешение DNS может использовать внутренний thread библиотеки/ОС, обещание нуля потоков не вводится.
- [ureq 3.4.2 TLS configuration](https://docs.rs/ureq/3.4.2/ureq/tls/struct.TlsConfigBuilder.html): default roots — WebPki. Системное доверие D-96 требует feature `platform-verifier` и явного `root_certs(RootCerts::PlatformVerifier)`; [RootCerts](https://docs.rs/ureq/3.4.2/ureq/tls/enum.RootCerts.html) подтверждает выбор rustls-platform-verifier, а не «TLS defaults preserved».

## Проверка спецификации

2026-10-21: builder-check `/root/builder_tk6` (gpt-6.1-sol/high) вернул `nothing important left` для rev1. После замечания пользователя о ненужной асинхронности rev2 переводит HTTP на синхронный клиент и процессный SIGINT; прежний результат не выдаётся за проверку rev2. Это проверка достаточности спецификации для плана, не ревью реализации. [План и задачи](../plans/2026-10-21-tk-6-embedding-transport.md) сохранены отдельно.

Rev2: независимый `/root/builder_tk6_sync` (gpt-6.1-sol/high) проверил синхронный вариант и соответствие процессного SIGINT контракту; результат `nothing important left`. Проверка реализации ещё не выполнялась.

Rev3: после rebase на cc82a8f независимые `/root/audit_tk6_architecture` и `/root/audit_tk6_wire` сверили изменённые входы TK-6. Исправлены: системное TLS-доверие, foreground cancelled/once-guard отдельно от drain, только HTTP 200, открытые parameters без слепой пересылки, отсутствие придуманного запрета query/fragment, обязательные поля config/response и границы платформенных доказательств. Старые builder-checks относятся к прежним редакциям, не доказывают соответствие новому D-96. Полный аудит дизайна пользователь отдельно исключил; transport tests и итоговый code review ещё не выполнялись.

## Результат реализации

8 Rust + 19 Python тестов на каждой ОС, 10 macOS / 11 Linux compiling negative controls, системный TLS и отдельный macOS preflight — PASS. Гонка cancelled/exit0 и ошибочный race oracle исправлены в одном RED→GREEN проходе после независимого ревью. G-0 — continue к TK-7. Начальные builder-checks и состояние открытого G-0 выше описывают историю планирования, а не текущую готовность. Границы inference/x86_64/производственной интеграции сохранены.
