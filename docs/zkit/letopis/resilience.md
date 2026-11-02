# letopis — устойчивость и безопасность

> **Links:** [requirements](requirements.md) · [resource-math](resource-math.md) · [contracts](contracts.md) · [data](data.md) · [architecture](architecture.md) · [mechanics](mechanics.md) · [decisions](decisions.md)
> **Status:** agreed · **Date:** 2026-10-21 · **Based on:** requirements.md (2026-10-21), resource-math.md (2026-10-21), contracts.md (2026-10-21), data.md (2026-10-21), architecture.md (2026-10-21), mechanics.md (2026-10-21)
> **Counts:** 9 F · 5 T


## Правка 2026-10-21

2026-10-21 ([D-95](decisions.md#D-95), [D-96](decisions.md#D-96)): сняты остатки E-12 и таблицы ошибок (данные — 10 E, всего 32 цели); drain использует attempts в embed_failures; DR-4 — висячие рёбра и циклы хранятся, не обходятся и видны в check/status; DR-8 — запись только в `.letopis/`, объекты Git и `refs/letopis/*`; SIGINT — обработанная отмена с ответом cancelled (D-96).
2026-10-20 ([D-95](decisions.md#D-95)): очередь embeddings выводится из текстов без вектора (embed_failures вместо заданий с seq), диагностика — текущие проблемы без истории и errors --clear, config.json правится как файл, reindex только полный без отмены по PID. F-2/F-7/F-8, O-2/O-5/O-6, DR-1/DR-4/DR-6 и шаг 5 процедуры восстановления переписаны; модель и численные сценарии не меняются.

2026-10-20 ([D-93](decisions.md#D-93), [D-94](decisions.md#D-94)): записи восстановления, отката и worker больше нет. F-1/F-2/T-2, O-2, DR-1/DR-4/DR-8 и процедура backup/restore переписаны: отказ во время публикации догоняется следующим вызовом, backup — Git/Markdown и config.json, права на `.letopis/` вместо runtime socket. [D-80](decisions.md#D-80)…[D-82](decisions.md#D-82) сохраняются.

2026-10-20 (D-92): векторы — в `.letopis/vectors.db`, переживают удаление кэша; потеря/повреждение vectors.db = повторное вычисление векторов без потери знаний. Backup его не требует.

2026-10-20 (D-91): служебная БД — пересоздаваемый кэш. Backup охватывает Markdown/Git и `.letopis/config.json` (+ recovery.json при наличии), БД не копируется; потеря или повреждение одной БД — удаление и reindex после recovery. Обновлены F-3, F-4, O-3, DR-2 и процедура backup/restore. Решения [D-80](decisions.md#D-80)…[D-82](decisions.md#D-82) сохраняются.
По [D-90](decisions.md#D-90) repair apply не скрывает внешний delta, status различает active/completed ошибки, inspect покрывает рёбра. DR-4/DR-6 проверяют эти пути; RAM/search drills используют явную current-vector coverage по resource-math. Новые drills/лимиты не вводятся.

## 1. Summary

Проект resilience дополняет согласованную mechanics реестром отказов/угроз, внешней процедурой восстановления и проверяемой моделью сценариев. Перебраны владельцы B-1…B-6, интерфейсы I-1…I-12, внешние X-1…X-4 и данные E-1…E-7, E-9…E-11. Принятые CLI/локальность не переоткрываются; после [D-95](decisions.md#D-95) seq и таблицы заданий нет; после [D-93](decisions.md#D-93) letopis не пишет корпус, записи восстановления и отката нет, после [D-94](decisions.md#D-94) нет постоянного процесса. Главный незакрытый вопрос — потеря единственной машины: Git-история не заменяет независимую резервную копию; служебная БД и vectors.db пересоздаются reindex и drain. Backup/restore в v1 поручены внешним инструментам и оператору по [D-80](decisions.md#D-80); по [D-81](decisions.md#D-81) фиксированного loss window после потери устройства нет, используется последняя пригодная внешняя копия; по [D-82](decisions.md#D-82) фиксированного restore deadline также нет. Модель считает явно иллюстративные сценарии, не обещание сроков.

Resource-math остаётся draft/not assessed, contracts — draft с прежними runtime-ограничениями. Продолжение resilience не объявляет их или P-1/P-2/P-3 выполненными. Новых измерений, inference, реализации и Git-коммитов нет.

### Фиксированный раунд R1

Независимый read-only gap-scan подтвердил ровно три независимых продуктовых решения. **T=3**, закрыто **3/3**, очередь новых вопросов **0**. Знаменатель не меняется при ответах; если ответ уже разрешает последующий вопрос, записываем результат без повторного формального вопроса.

| Gap | Выбор | Что меняется | Состояние |
|---|---|---|---|
| G1 | Внешний оператор/tool выполняет backup и restore | [D-80](decisions.md#D-80): описать coherent copy/restore procedure, без встроенных команд или сервиса | closed |
| G2 | Нет фиксированного loss window после permanent storage failure | [D-81](decisions.md#D-81): восстановить последнюю пригодную внешнюю копию; возраст не гарантирован, отсутствие копии может сделать потерю невосстановимой | closed |
| G3 | Нет фиксированного restore-time deadline v1 | [D-82](decisions.md#D-82): корректное восстановление до graph/FTS service, без ожидания новых vectors | closed |

Не задаём вопросы ради новых компонентов: no ACL, одна машина без постоянного процесса, local default, CLI-only, polling status, явный remote provider и бессрочные Git snapshots уже решены. Конкретные failure-rate/скорости диска/restore не выдаём за измерения. Provider I/O deadline, fail-closed при corruption, секреты, формат restore validation и drills — технический дизайн с явными предположениями и проверками.

## 2. Faults

| ID | Target | Failure | Severity | Detection | Response | Recovery | Serves | Residual |
|---|---|---|---|---|---|---|---|---|
| F-1 | B-1, B-2, B-6, I-1, I-3, I-5, E-9 | Процесс CLI или drain умирает, теряется ответ | H | O-1, O-2 | Ядро снимает flock; незафиксированная unit откатывается; следующий вызов повторно обнаруживает изменения; команды не меняют корпус, повтор идемпотентен | MC-2/MC-5, DR-1 | FR-32, NFR-1 | [TK-4](checks/TK-4/findings.md): bounded process-failure/SIGINT schedules PASS на macOS/Linux ARM64; physical power loss и production drills ещё P-3 |
| F-2 | B-2, B-3, I-2, X-3, E-1, E-2, E-3 | Потеря питания во время публикации или reindex; незавершённая агентом группа правок или правка config.json | H | O-2, O-3 | Кэш — одна SQLite unit с files-строками; недописанный config.json — диагностика и FTS-only до исправления; следующий вызов публикует дельту; незавершённая группа видна как висячие ссылки | DR-1, NFR-1 при исправных durable primitives | FR-6, FR-13, NFR-1 | Сохранность правок агента зависит от его инструментов и Git; модель не доказывает hardware flush |
| F-3 | B-2, B-3, E-1, E-2, E-3, E-6, E-7, E-10 | Полная потеря диска/устройства; потеря одной служебной БД | H | O-1, O-3, O-7 | Устройство: восстановить Markdown/Git и `.letopis/config.json` из независимой копии. Только БД: удалить её и выполнить reindex (D-91); векторы неизменных текстов берутся из `.letopis/vectors.db` (D-92) | DR-2; [D-80](decisions.md#D-80)/D-81/D-82 | FR-1, FR-17, FR-33, FR-34, FR-35 | За независимую копию отвечает оператор по [D-80](decisions.md#D-80); нет обещанного maximum loss window или restore deadline по [D-81](decisions.md#D-81)/D-82 |
| F-4 | B-2, B-3, I-12, X-3, E-2, E-3, E-5 | Повреждённые Git objects/refs, SQLite-кэш или vectors.db | H | O-2, O-3 | Git: fail closed, сохранить набор для анализа. Кэш/vectors.db: удалить файл и выполнить reindex (D-91, [D-92](decisions.md#D-92)) | DR-2, DR-3; корректная копия | FR-17, FR-34, FR-35 | Повреждение Git без читаемой копии reindex не исправляет; повреждение кэша/vectors.db исправляет |
| F-5 | B-2, B-3, B-4, B-5, I-2, I-4, I-7, I-8, X-4, E-1, E-3, E-4, E-5 | Невалидная внешняя Git-правка, исчезнувший источник, рассогласованная производная проекция | H | O-3, O-4 | Последний корректный индекс; flags и диагностика; только валидная публикация, согласованный snapshot читателю | DR-3, DR-4 | FR-6, FR-20, FR-21, FR-27, FR-28, NFR-7 | Регистрировать обычную недоступность источника, не объявлять её утратой знания |
| F-6 | B-3, B-4, I-6, X-2, E-5, E-6, E-7, E-11 | Модель/endpoint завис, ключ истёк, квота, неверный ответ/размерность | H | O-5 | Сохранённые files/graph/FTS остаются; конечный provider I/O deadline и ограниченные retries; query → FTS; ключ не в диагностике | DR-5, FR-30 budget | FR-13, FR-22, FR-29, FR-30, NFR-6 | Число попыток не ограничивает длительность зависшего запроса без I/O deadline; assumed 60s per-attempt baseline по [D-83](decisions.md#D-83) проверяется DR-5 |
| F-7 | B-3, I-6, I-10, E-6, E-7 | Старый result после правки/config/SIGINT, restart сбрасывает бюджет | H | O-4, O-5, O-6 | Drain перечитывает файл и считает вектор только при совпавшем search_hash; попытки в embed_failures резервируются до запроса; исчерпанные не возобновляются автоматически; смена config сбрасывает vectors.db одной транзакцией, drain сверяет meta.config_identity перед записью вектора | DR-6 | FR-29, FR-30, FR-31, FR-32 | Интеграционный vec1 reset/readers ещё не пройден |
| F-8 | B-2, B-3, B-4, I-8, I-9, I-10, I-11, E-4, E-5, E-7, E-10 | ENOSPC/OOM/долгий reader и рост WAL, ошибка записи диагностики | H | O-2, O-6 | Не удалять Git refs ради места; незафиксированная публикация откатывается; JSON ошибки при возможности, не обещать durable log на недоступном диске | DR-1, DR-7 | FR-33, NFR-1, NFR-4, NFR-7 | Не все ошибки можно сохранить на отказавшем носителе; SIGKILL может не дать JSON |
| F-9 | B-1, B-2, E-1, E-3, X-3 | Ошибка wall clock, несовместимая сборка/расширение или запуск на непроверенной FS | H | O-1, O-3, O-8 | Проверка capabilities; не обслуживать несовместимую запись, elapsed на monotonic; даты знаний не переписывать | DR-7, DR-8 | FR-35, NFR-5 | Достоверность wall clock сама по себе M; общий H относится к нарушению Must platform/durability |

## 3. Threats

| ID | Target | Threat | Severity | Detection | Response | Recovery | Serves | Residual |
|---|---|---|---|---|---|---|---|---|
| T-1 | B-1, B-2, I-1, I-4, I-12, X-1, X-3, X-4, E-1, E-2 | Вход агента выводит путь за Git-root, symlink/argv/Markdown injection | H | O-3, O-8 | Строгий JSON/тип OID/path, argv без shell, root-boundary и чтение только обычных файлов; конечные не-regular записи корпуса пропускаются без перехода по symlink, подмена при чтении — ошибка; не писать в код/неразмеченные документы | DR-8 | FR-3, FR-6 | Произвольный вредоносный процесс с правами пользователя вне sandbox не изолирован приложением |
| T-2 | B-1, I-1, I-3, I-5, I-6, E-9 | Другой OS user читает кэш, подменяет lock-файлы или посылает сигнал | H | O-1, O-8 | Каталог `.letopis/` 0700, файлы 0600; владение блокировкой — только ядро; SIGINT — обработанная отмена собственного процесса с ответом cancelled ([D-96](decisions.md#D-96)), CLI не сигналит чужому PID ([D-95](decisions.md#D-95)); PID в lock-файле только для диагностики | DR-8 | NFR-2, NFR-5, C-1 | Same-UID процессы входят в доверенную границу; отдельный agent ACL не добавляется; [TK-4](checks/TK-4/findings.md) не является hostile-client drill |
| T-3 | B-3, B-4, X-2, E-6, E-11 | Ключ или knowledge уходят в endpoint/log/backup, вредоносный provider response | H | O-5, O-8 | Явный provider/base_url; local path без remote request; env key, редактирование diagnostics, проверка vectors/response; не исполнять текст ответа | DR-5, DR-8 | FR-22, NFR-6, C-6 | У выбранного remote сроки хранения неизвестны; offline гарантия относится к local mode |
| T-4 | B-2, B-4, B-5, I-4, I-7, I-8, X-4, E-1, E-3, E-4, E-5 | Агент сохраняет правдоподобное ложное знание/prompt injection в корректной разметке | H | O-4, O-8 | Источник/снимок/flags и чтение агентом как данных; валидатор проверяет структуру, не истину; сохранять противоречия | DR-4, DR-8 | FR-1, FR-4, FR-18, FR-20, FR-21 | Автозапись и равные права уже приняты; semantic poisoning невозможно исключить одним graph validator |
| T-5 | B-1, B-3, B-4, X-1, X-2, X-4, E-4, E-5, E-7, E-10 | Огромный вход/глубокий граф/медленный provider истощают память/время/диск | H | O-5, O-6 | Конечный I/O deadline, отмена и итеративный обход, последовательные тексты, безопасный отказ/восстановление; без новых произвольных hard quotas | DR-7, DR-8 | FR-26, FR-30, FR-31, NFR-4 | Цель RAM и mixed-client workload ещё P-2; враждебный same-UID writer может DoS весь проект |

## 4. Security and isolation design

Границы: агент ↔ процесс CLI; пользовательский ввод/Markdown ↔ кодек/валидатор; процессы letopis ↔ Git/SQLite/filesystem; drain/search ↔ выбранная embedding-модель. B-1…B-6 — модули одного бинарника, не изоляция от компрометации друг друга. Новые tenants, агентские роли/ACL, облачная репликация, sandbox-сервис и шифрование ключей в собственной БД не вводятся.

OPENAI_API_KEY приходит из окружения процесса letopis (drain наследует окружение вызвавшего процесса) и не попадает в JSON parameters/stdout/status/Git/SQLite. Новый ключ действует со следующего вызова CLI и следующего запуска drain, собственное secret storage не строится. Все local agents имеют доступ к общему корпусу по принятой границе. Компрометированный процесс того же OS пользователя нельзя остановить chmod каталога `.letopis/`; это явный остаточный риск платформенной границы, не скрытая гарантия ACL.

Удаление из текущего графа не стирает бессрочную Git-историю и возможные внешние копии. Backup/restore выполняет оператор внешними средствами по [D-80](decisions.md#D-80); по [D-81](decisions.md#D-81) фиксированного срока допустимой потери нет; автоматический purge исторических knowledge/snapshots не появляется. Факты про механизм FS/Git/SQLite берём из существующих R-2/R-8; никаких новых утверждений о vendor availability или failure rates не добавлено.


### Практические границы без новых сервисов

Проектный embedding transport по [D-83](decisions.md#D-83) ограничивает каждое обращение внутренним client deadline 60s. Это budget ожидания одной попытки, не cutoff всего search/reindex и не новый параметр пользователя. По deadline клиент завершает свою попытку с embedding_failed; query выполняет обычный FTS, фоновый drain резервирует попытку в embed_failures и соблюдает max_retries ([D-95](decisions.md#D-95)). Ни отсутствие embeddings, ни их timeout не откатывают files/graph/FTS. Нельзя обещать физическое прекращение работы remote/Ollama после разрыва клиентского соединения; поздние результаты теряют право публикации. DR-5 проверяет реальное освобождение ресурсов/FTS fallback, включая медленную локальную модель.

Redirects и неявные environment proxies отключены, HTTPS certificate verification не обходится. Адрес endpoint выбран в конфигурации; HTTP за пределами доверенной loopback-среды не даёт конфиденциальности транспорта. Смена/отзыв ключа выполняется у provider и вступает в силу со следующего запуска drain (окружение процесса CLI, drain наследует окружение запустившего вызова); исчерпанные попытки (embed_failures) не возобновляются сами; повтор — после смены конфигурации или удаления vectors.db. Provider body/headers/error.message не копируются в диагностику: локальный безопасный текст и код класса/статуса достаточны, в частности при отражённом промпте/ключе. Arbitrary user text в knowledge может сам содержать секрет; приложение не обещает универсальный DLP scanner.

Другой OS user не должен открыть служебную БД, vectors.db или lock-файлы: каталог `.letopis/` 0700, файлы 0600 с корректным umask; это OS boundary, не agent ACL. Репозиторий и его внешние backup-копии защищаются правами оператора; letopis не меняет рекурсивно чужие repository permissions. Процесс того же UID, включая вызывающего агента, имеет доверенный доступ и может испортить данные/сменить provider; эти действия не выдаются за изолированные. Правки через Git — штатный путь [D-93](decisions.md#D-93): они обнаруживаются следующим вызовом как обычные изменения файлов.

FR-36 agent skill должен трактовать найденный Markdown как свидетельство/данные, а не инструкции выше пользовательской задачи; проверять source_snapshot/актуальность, сохранять противоречащие знания отдельно и перечитывать состояние после lost reply. Структурная валидация и stable IDs уменьшают ошибки ссылок, но не исключают valid semantic poisoning. Остаточный риск следует из [D-1](decisions.md#D-1)/D-2 о самостоятельной записи с общими правами, а не из несуществующего детектора истинности.

### Внешняя согласованная copy/restore-процедура

Это минимальные обязательные условия документации по [D-80](decisions.md#D-80), не подробный runbook и не новые Letopis commands:

1. **Зафиксировать quiet project.** Оператор приостанавливает агентов/новые CLI и внешний Git, дожидается завершения процессов letopis, включая drain (embed.lock свободен); подтверждает отсутствие writers на весь период копирования. Вызовы letopis во время копирования не запускаются. Внешний tool, который не может обеспечить эту границу или согласованный snapshot всего набора, не объявляется пригодным для гарантированного restore. API live backup SQLite само по себе не координирует Markdown/Git.
2. **Копировать один полный набор.** Рабочая копия Markdown и нужные исходные файлы; реальное Git-хранилище objects/refs включая private refs/letopis; `.letopis/config.json`. Служебная БД и `.letopis/vectors.db` не копируются: это пересоздаваемые данные (D-91, [D-92](decisions.md#D-92)); копирование vectors.db допустимо лишь как экономия повторных вычислений; если её всё же копируют, то только вместе с её WAL, иначе её удаляют при restore. Git-dir может быть отдельно от worktree: копируются реальные dependencies, а не только файл-указатель .git. Зависимость от отсутствующих alternate objects делает копию непригодной. Ключ из окружения не становится app-export data; секреты, самостоятельно записанные пользователем в репозиторий, остаются ответственностью его внешнего backup policy.
3. **Проверить независимость и читаемость.** Другая директория на том же отказавшем носителе не является независимой защитой от его утраты. Оператор проверяет self-contained Git object closure/refs и читаемость config.json на копии, фиксирует её coherent recovery point средствами внешнего tool. Пригодность копии доказывается DR-2, не одним фактом существования файла. Частоту/срок хранения letopis не назначает. [R-9](research/R-9-sqlite-copy-and-restore.md).
4. **Restore только offline.** Новый изолированный target; никакого подменивания файлов под работающими процессами letopis. Возвращаются Git/Markdown и файлы `.letopis/` одной копии; кэш строится заново. Старые lock-файлы безвредны: владение определяет ядро; работающие процессы не переносятся. Несовместимая сборка/extension не записывает новую БД поверх старой.
5. **Верифицировать durable state, затем обслуживать.** Проверить Git closure включая принимаемые source_snapshot и config.json. Первый вызов строит отсутствующий кэш полным reindex: граф/FTS, source и их created_at из ссылок, тексты без вектора в vectors.db drain найдёт запросом (D-91, [D-92](decisions.md#D-92), [D-95](decisions.md#D-95)). При corruption Git — fail closed и другая пригодная копия.
6. **Признать recovery point.** После корректного запуска graph/FTS доступны, embeddings могут отставать. Все изменения после точки копии, включая удаления, могли быть утрачены: старые знания могут вернуться. Диагностика после restore вычисляется заново (кэш пересоздан, [D-91](decisions.md#D-91)); счётчики попыток лежат в vectors.db ([D-95](decisions.md#D-95)) и начинаются заново, потому что vectors.db не копируется (если её скопировали, они сохраняются): provider calls могут повториться. Обычный restart не сбрасывает бюджет, но disaster restore не гарантирует exactly-once для внешних вызовов. Это следствие [D-81](decisions.md#D-81) и бессрочной Git-истории, не обещание гарантированного стирания. Источник outage фиксирует оператор; собственная копия, создавшаяся на той же машине, не доказывает её сохранность после device loss.

Ни при ENOSPC, ни при corruption приложение не удаляет retained refs или embed_failures автоматически для освобождения места. Если публикацию или диагностику записать нельзя, CLI возвращает безопасный JSON internal_error при возможности; кэш остаётся в прежнем согласованном состоянии. Ошибка, полученная на недоступном носителе, не объявляется надёжно сохранённой; оператор устраняет storage fault и повторяет обычный старт. Без рабочего storage соблюдение конечного срока восстановления не обещано по [D-82](decisions.md#D-82).

## 5. Observability

| ID | Signal | Detects | Emitted by | Alert / threshold | Kept for |
|---|---|---|---|---|---|
| O-1 | Ошибка запуска процесса CLI, открытия кэша или flock, JSON ошибки запуска | F-1, F-3, F-9, T-2 | B-1 | Текущий вызывающий агент при отказе; без отдельного heartbeat/email | Один ответ; хранимой ошибки запуска нет, текущие проблемы видит status |
| O-2 | Исход публикации/reindex/config и storage error | F-1, F-2, F-4, F-8 | B-2, B-3 | CLI получает безопасную ошибку; незафиксированная публикация повторяется следующим вызовом | Пока причина существует ([D-95](decisions.md#D-95)) |
| O-3 | Проверка объектов Git, разметки по файлу, графовой целостности и paths | F-2, F-3, F-4, F-5, F-9, T-1 | B-2, B-3 | При каждом обнаруженном несоответствии; отсутствие проверки не называется отсутствием corruption | Доступная диагностика по MC-7; внешние копии оператора по [D-80](decisions.md#D-80) |
| O-4 | Revision, stale/source flags, provenance | F-5, F-7, T-4 | B-4, B-5 | Search/inspect/traverse текущему агенту, не подтверждение истинности | Ответ и текущие проблемы status; без нового audit archive |
| O-5 | Provider deadline/error, attempts и безопасный код | F-6, F-7, T-3, T-5 | B-3, B-4 | Ошибка query — обычный FTS-ответ без пометки и последняя строка vec.query_error в status до успешного запроса (D-62); ошибка запуска drain — только предупреждение; исчерпавшие попытки тексты показаны отдельно от отставания | Пока строка embed_failures существует |
| O-6 | Status: занятость блокировок с PID, число текстов без вектора, ошибки открытия DB | F-7, F-8, T-5 | B-3, B-6 | Агент опрашивает status; отсутствие обещанного drain SLO не создавать как alert threshold | Вычисляется при каждом status из кэша и vectors.db |
| O-7 | Проверка наличия/читаемости независимой копии и restore receipt | F-3, F-4 | Оператор/внешний tool по [D-80](decisions.md#D-80); B-1/B-2 только проверка кэша при вызове и reindex | Проверка копии/восстановления оператором; нет заданного срока свежести копии по [D-81](decisions.md#D-81), новой restore-команды нет | Внешние записи оператора; срок и частота ещё не обещаны |
| O-8 | Негативные проверки путей/секретов/lock-файлов/fixtures, OS clock и сборки | F-9, T-1, T-2, T-3, T-4, T-5 | B-1, B-2, B-3 | Drill/pilot и текущая ошибка; тесты не считаются continuous intrusion detector | Отчёт drills, не новый runtime event store |

Нет push alerts/автоматизаций, новых внешних сообщений или логирования полных патчей. При полной потере машины её status не является обнаружением: это неуспешный запуск/наличие независимой копии у оператора. Атаку семантическим poisoning структурный validator не обнаруживает гарантированно; flags только помогают перепроверке.

## 6. Drills

Ни один drill сейчас не выполняется: это вход roadmap/P-1/P-2/P-3, не новые замеры.

| ID | Covers | How | Success criterion | Becomes |
|---|---|---|---|---|
| DR-1 | F-1, F-2, F-8 | Fault injection: kill процесса на каждом шаге публикации, reindex и сброса при смене config; отдельный физический power-loss тест на macOS/Linux | Обслуживаемое состояние всегда — последняя зафиксированная публикация; следующий вызов публикует оставшуюся дельту; корпус не изменён letopis; failure = пропущенное изменение или смешанный кэш | roadmap P-3 |
| DR-2 | F-3, F-4 | Restore на чистой машине из выбранной независимой копии без доступа к исходному диску | Проверены Git blobs/refs и config.json; кэш пересобран reindex, source и created_at совпадают с выведенными из ссылок; восстановлен согласованный recovery point копии по [D-81](decisions.md#D-81) без обещанной максимальной давности; restore deadline не задан по [D-82](decisions.md#D-82); failure = утрачен Git closure или config.json | roadmap P-3 |
| DR-3 | F-4, F-5 | Удалить/испортить Git object, отдельно весь SQLite-кэш и vectors.db | Повреждённый Git — fail closed; удалённый или испорченный кэш пересобирается reindex с теми же source ID/created_at из ссылок (D-91); невалидная публикация не заменяет последнюю корректную; failure = тихий reset/успех на неполных данных | roadmap P-3 |
| DR-4 | F-5, T-4 | Git checkout/недописанная разметка/удалённый source; невалидный A рядом с валидно изменённым B; висячая ссылка; includes-цикл из двух файлов; перепроверка с новым source_snapshot | B публикуется, A сохраняет прежнюю проекцию с активной диагностикой; висячее ребро и цикл хранятся, не обходятся и видны в check/status; исправление A убирает диагностику из status; flags/refs стабильны | roadmap P-1/P-2/P-3 |
| DR-5 | F-6, T-3 | Endpoint не отвечает/неверная длина/401/429, ключ-канарейка, local mode без внешней сети | Assumed client deadline 60s освобождает ожидание (фактическую задержку фиксирует drill); SIGINT отменяет search ответом cancelled, а drain завершает по умолчанию без ответа с сохранёнными attempts ([D-96](decisions.md#D-96)); FTS доступен, attempts bounded, ключ отсутствует во всех артефактах; failure = зависший drain/утечка/несовместимый vector | roadmap P-2/P-3 |
| DR-6 | F-7 | Правка/config/удаление и restart между reservation/compute/публикацией вектора; два процесса на index.lock и SIGINT | Поздний result хранится только под своим search_hash и не представляет новый текст; restart не пополняет бюджет attempts; cancellation соответствует X-1; исчерпавшие попытки видны в status при нулевом отставании | roadmap P-2/P-3 |
| DR-7 | F-8, F-9, T-5 | Disk full, allocation failure, долгий reader/WAL и глубокий граф, несовместимая extension build | Controlled failure или recoverable crash без false success и смешанного кэша; bounded retry; отсутствие RAM/latency verdict до измерения | roadmap P-2/P-3 |
| DR-8 | F-9, T-1, T-2, T-3, T-4, T-5 | Path/symlink/argv/lock-file fixtures (включая пропуск конечных не-regular записей и отказ при подмене), malformed JSON/Markdown/provider, bad clock, valid malicious prose | letopis пишет только в `.letopis/`, объекты Git и `refs/letopis/*` и не меняет корпус; другой OS user не читает `.letopis/`; запросы модели не исполняются; same-UID/semantic residuals названы, а не объявлены устранёнными | roadmap security drills |

## 7. Assumptions

Ни одна RA-n ниже не измерена и не является обещанием. Это **синтетические входы арифметического what-if примера**, явно chosen by author по [D-83](decisions.md#D-83); колонка expected в стандартном checker означает только средний демонстрационный сценарий, не ожидаемую статистику эксплуатации. Сценарий с usable_copy=0 специально показывает отсутствие конечного восстановления. Эти значения нельзя использовать для обещания RPO/RTO/availability или sizing backup storage. Реальные значения вводятся оператором/DR-2 после измерения, если понадобится оценка; сейчас новых замеров нет.

| ID | Assumption | Unit | Best | Expected | Worst | Evidence | Source | Validation |
|---|---|---|---|---|---|---|---|---|
| RA-1 | Синтетическое число storage-loss incidents в год | 1/year | 0 | 1 | 4 | assumed | Пример автора [D-83](decisions.md#D-83), не статистика дисков | Внешний incident log при желании оценивать availability; нет SLA |
| RA-2 | Синтетическая задержка обнаружения/реакции оператора | h | 0 | 1 | 24 | assumed | What-if fixture, не heartbeat deadline | Измерить операторский процесс в DR-2, отдельно от app restore |
| RA-3 | Синтетическая подготовка исправного устройства/usable copy | h | 0 | 24 | 168 | assumed | What-if fixture, не срок поставки оборудования | Записать фактическую задержку отдельно при DR-2 |
| RA-4 | Синтетическое restore до graph/FTS после готовности hardware/copy | h | 0.1 | 1 | 24 | assumed | What-if fixture, не benchmark корпуса | DR-2 на реальном согласованном наборе, vectors не ожидаются |
| RA-5 | Синтетический возраст coherent recovery point копии при потере | h | 0 | 24 | 168 | assumed | What-if fixture; 24h не является backup interval v1 | Внешний tool/copy receipt; [D-81](decisions.md#D-81) не даёт верхней границы |
| RA-6 | Пригодная независимая копия доступна | bool 0/1 | 1 | 1 | 0 | assumed | What-if fixtures с/без copy; продукт не создаёт её | DR-2 без доступа к исходному диску, проверить все durable данные |

## 8. Model

resilience/model.py — plain arithmetic, stdlib, не resource benchmark. Для usable copy: scope_restore_h = RA-4; incident_to_service_h = RA-2 + RA-3 + RA-4; lost_window_h = RA-5. При отсутствии пригодной copy времена/окно полной согласованной recovery не имеют конечной границы (`inf`). Это не утверждение, что каждый отдельный Markdown потерян: не обещано восстановление полного durable state.

Иллюстративный downtime/month = min(8760, incidents/year × incident_to_service_h) / 12, availability = 100 × (1 − downtime/year / 8760). Это упрощённый **условный** расчёт без корреляции/накопления outages, только для демонстрации входов; не production reliability forecast. При incidents=0 downtime=0, даже если нет копии; при incidents>0 и невозможном восстановлении сценарий capped at whole-year outage. Скрытый provider outage не считается storage incident; FTS degradation не равна полной недоступности.

<!-- model:begin -->
**Results**

| Result | Unit | Best | Expected | Worst |
|---|---|---|---|---|
| illustrative_restore_scope_h | h | 0.1 | 1 | inf |
| illustrative_incident_to_service_h | h | 0.1 | 26 | inf |
| illustrative_loss_window_h | h | 0 | 24 | inf |
| illustrative_downtime_h_month | h/month | 0 | 2.167 | 730 |
| illustrative_availability_pct | % | 100 | 99.7 | 0 |

**Limits**

| ID | Metric | Limit | Best | Expected | Worst |
|---|---|---|---|---|---|

**Sensitivity** (one assumption at its worst value, others expected; the limit it moves most)

| Assumption | Limit | Change of the metric |
|---|---|---|
<!-- model:end -->

Исходный RA-4 включает offline copy, проверку Git и полный reindex graph/FTS, но не acquisition hardware/copy (RA-3), operator wait (RA-2) или drain всей embedding queue. Нельзя складывать одну и ту же фазу дважды либо показывать сценарий expected как обещанный one-hour restore.

## 9. Limits and verdicts

**Количественных service limits RL-n нет намеренно:** [D-81](decisions.md#D-81) исключает фиксированный permanent-loss RPO, [D-82](decisions.md#D-82) — restore-time deadline, availability SLA раньше не вводился. Поэтому `LIMITS=[]`; нулевое число failing limits не является verdict fits. Реальные recovery time/availability/RPO **not assessed**; above figures — только what-if. Вместо нового backlog SLO/status поля остаётся existing status.

NFR-1 — обязательный инвариант после [D-93](decisions.md#D-93): прерванная публикация не оставляет смешанного кэша и догоняется следующим вызовом; letopis не меняет корпус. NFR-7 — один согласованный read snapshot. Они проверяются моделями MC-2/MC-3/MC-5/MC-7 и физическими DR-1/DR-3/DR-6; искусственный numeric “0h loss” из константы модели не подменяет эти проверки. NFR-6 local mode проверяется DR-5, NFR-4 RAM — DR-7/P-2. Успех проверки арифметики не объявляет ни один runtime Must выполненным.

## 10. Not applicable

Целей B/I/X/E без риска нет: все 32 (6 B + 12 I + 4 X + 10 E) представлены в реестре. Отдельно не применимы multi-tenant access control, cloud failover, несколько worktrees, HTTP/MCP-сервер letopis и скрытый сетевой telemetry channel: они исключены из v1. Это не освобождает от проверки root/OS boundary/секретов.

## 11. Reconciliation with earlier steps

[D-80](decisions.md#D-80)…[D-82](decisions.md#D-82) фиксируют выбранный минимальный operational scope; публичные JSON schemas/CLI и lifecycle data не меняются. [D-83](decisions.md#D-83) отдельно отмечает инженерные assumptions и их проверки. Полная потеря БД и независимая копия уже были отложены в data/architecture/mechanics; собираем их в одну явную границу. По [D-80](decisions.md#D-80) встроенные backup/restore исключены из v1; документация задаст состав согласованной внешней копии и проверку восстановления. Это не новая CLI-команда, расписание или автоматическая отправка данных.

## 12. Risks, assumptions and open questions

R1 закрыт 3/3 по [D-80](decisions.md#D-80)…[D-82](decisions.md#D-82), новых вопросов в очереди 0. Принятые остаточные риски: потеря новее внешней копии/отсутствие пригодной копии и неограниченный restore time. Сохраняются ранее принятые одна машина, общая trust boundary без agent ACL и самостоятельное накопление знаний; невозможно объявить same-UID adversary/semantic poisoning устранёнными. У выбранного remote provider сроки копий неизвестны; локальный режим остаётся обязательным. Эти границы не добавляют новый hosting/backup продукт.

Непроверенные assumptions [D-83](decisions.md#D-83): client deadline/cancel release — DR-5; no redirect/proxy/key leaks/root/lock-file containment — DR-8; coherent copy/restore и full DB corruption — DR-2/DR-3; flock/SQLite unit/extension — DR-1/DR-7. What-if inputs не двигают verdict относительно отсутствующих RL и не являются исходными эксплуатационными данными. Для фактической оценки каждый RA имеет источник измерения/validation в таблице; до него реальные цифры not assessed.

## 13. Deferred

→ roadmap: выполнить DR-1…DR-8 с P-1/P-2/P-3; resource/RAM/drain и физические измерения здесь не запускаются.

## 14. Consistency check

2026-10-19: интервью R1 закрыто 3/3, очередь 0; clarity gate выполнен, статус agreed относится к логическому дизайну. Все 34 targets покрыты, каждый H имеет signal/drill; generated model block и 17 арифметических проверок проходят. Независимый closeout review: consistent, материальных замечаний 0. Повторно проверены предыдущие этапы, внешние примеры и внутренние модели с guard mutations. Отчёт — [checks/resilience/resilience-review-2026-10-19.md](checks/resilience/resilience-review-2026-10-19.md); протокол — [checks/resilience/resilience-checks.json](checks/resilience/resilience-checks.json). Физические drills и ресурсные измерения не запускались; resource-math остаётся draft/not assessed. Следующий этап — roadmap, он автоматически не начинается.

## Sources

Текущие requirements/data/architecture/mechanics/decisions и [R-8](research/R-8-durability-and-search-primitives.md). [R-9: SQLite copy/restore](research/R-9-sqlite-copy-and-restore.md). Это проектный register на основе принятых границ, не результаты эксплуатации или исследования failure rates.
