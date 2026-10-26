# TK-25 — полный reindex и обнаружение Git-изменений

**Состояние:** дизайн по карточке; пользователь выбрал «По карточке», пропустив уточнения. Описанные ниже детали реализации — выбор исполнителя. Пользователь утвердил план и Native исполнение; Task 1/2 реализованы и host-проверены; независимый whole-branch review `913b2b7..0dfe83c` принят без замечаний (Ready to merge Yes). Независимый builder check `/root/tk25_builder_check`: `nothing important left`.

**База:** `main` / `origin/main` на момент создания worktree: `913b2b7c043fa9b8df953b323659a73e74bdb742`; ветка `tk-25`, отдельный managed worktree. На этой чистой базе `make check` завершился успешно; журнал `/tmp/tk25-baseline-20261026.log`. Старые evidence не перезаписываются.

## Основания и цель

- [Карточка TK-25](../../zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-25-reindexing.md).
- [MC-2 / MC-5](../../zkit/letopis/mechanics.md), [I-2 публикация](../../zkit/letopis/contracts/I-2-projections.md).
- [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md), [машинная схема](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [D-71 / D-95 / D-97 / D-101](../../zkit/letopis/decisions.md), [FR-27 / FR-28 / FR-34 / C-8](../../zkit/letopis/requirements.md).
- [Структура и проверка границ](../../zkit/letopis/architecture.md#program-structure), [AGENTS.md](../../../AGENTS.md).

Добавить явную CLI-команду полного `reindex`, завершить тот же путь для первого построения и доказать обнаружение внешних Git-изменений обычными обновляющими командами. Использовать существующий синхронный Rust, SQLite WAL, application-owned traits и Git argv; новые зависимости, runtime и процессы не нужны.

Пилот TK-17, ресурсная приёмка и G-1 не объявляются пройденными. Пользователь явно запросил разработку TK-25; это не release-решение и не снятие критериев gate. TK-24, TK-10 и TK-13 уже интегрированы в базу; последняя также включает новые observational `status` и диагностику `check`.

## Текущее поведение

`DocumentRefresher::refresh` перечисляет Git-кандидатов, использует stat/hash предфильтр, повторяет обнаружение под `index.lock`, последовательно готовит и публикует файлы одной cache write unit. `prepare_file` при прежнем hash сохраняет проекцию без разбора; при ошибке Markdown сохраняет last-valid проекцию, обновляя `files.error`. Graph/FTS и files публикуются вместе, revision увеличивается один раз. Отдельной команды reindex пока нет.

`SqliteStorage` хранит contentless FTS5: прежний search text нельзя прочитать для повторной вставки невалидного файла. Поэтому глобальный DELETE/DROP перед разбором нарушил бы критерий last-valid. Пересоздание физических таблиц и восстановление повреждённой БД не являются задачей TK-25.

## Контракт CLI

`letopis reindex` принимает пустой JSON-объект `{}` через существующий input transport. Лишние аргументы, selective paths, `--cancel`, неизвестные поля и malformed JSON отклоняются до создания сервисов. Ответ по существующей X-1 схеме: успешный envelope с `result: {"revision": N}`, один JSON, exit 0. Никаких текстов корпуса, прогресса, operation ID или новых полей.

Публикация при явном reindex выполняется даже для неизменённого или пустого корпуса: revision увеличивается один раз. Повторный reindex сохраняет логическое содержимое, но возвращает следующий publication counter. При сбое — прежний безопасный typed failure, exit 1, без частичного успешного результата и payload в диагностике.

После успешного commit существующий BackgroundRunner может запустить отсоединённый drain. Reindex не вызывает inference, не ждёт embed.lock, ответа модели или завершения drain. Ошибка запуска отражается существующим warning и не отменяет завершённую публикацию. Обычная готовность embedding/config остаётся в её нынешнем владельце; новая lifecycle-логика не вводится.

## Один владелец публикации

Расширить существующую capability `Refresher` операцией полного reindex; реализовать её на том же `DocumentRefresher`, с общими подготовкой и публикацией I-2. Не создавать pass-through ReindexService или отдельный алгоритм публикации.

`refresh` сохраняет текущий incremental fast path: пустая дельта не вызывает write unit или index.lock; изменение одного файла не перерастает в reindex. При отсутствии опубликованной ревизии initial build использует полный путь после получения lock. После ожидания lock состояние перечитывается: если другой процесс уже завершил первичную публикацию, обычный refresh заново определяет только текущую дельту. Явный reindex всё равно остаётся полным.

Полная операция:

1. Получить устойчивый kernel `index.lock` до перечисления/подготовки полного корпуса. Держать guard до commit либо ошибки; не удалять lock-файл.
2. Под lock прочитать текущую revision, files и необходимую graph metadata, перечислить текущих Git-кандидатов. Tracked missing paths — удаления; ignored untracked не входят; tracked ignored продолжают входить. Сохранить confinement и запрет следования symlink.
3. Выбрать все существующие кандидаты независимо от прежнего stat/hash. Полный режим принудительно повторно разбирает даже hash-equal валидные и невалидные файлы. Накопление bytes/search text всего корпуса запрещено; временные bytes принадлежат одному обрабатываемому файлу, metadata может храниться как в существующем пути.
4. Выполнить существующий provenance preflight/retention для используемых оснований, в том числе hash-equal знаний при полном режиме. Сохранить typed ошибки и проверку фактических захваченных stat/hash. Не записывать новый source_snapshot вместо авторского, не менять Markdown/Git HEAD/index. Retention добавляет только существующие служебные refs по действующим правилам.
5. В одной caller-owned cache write unit обработать удаления и все выбранные файлы через общий per-file projection/publish. Для каждого валидного файла заново заменить его nodes/outgoing edges/FTS и files; у unmarked файла не остаётся прежних знаний. Неизменные валидные файлы также действительно переиндексируются.
6. Для невалидного файла оставить прежние nodes/edges/FTS и source_snapshot, записать текущий fingerprint и безопасную ошибку в files. Если прежней проекции нет, никаких узлов не создавать. Невалидность A не прекращает обработку валидного B. Новый stat/hash A не должен скрыть необработанное изменение B.
7. Пересогласовать source metadata по итоговому графу, включая исходные source-узлы, затронутые удалениями/полным reindex: удалить потерявшие последнюю ссылку, сохранить missing sources с null hash, воспроизвести created_at по текущим ссылкам, не fetch HTTP/HTTPS. Сохранить нормальные incoming dangling edges и query-time includes-cycle semantics.
8. Commit увеличивает revision ровно один раз; освободить lock до фонового запуска. Если подготовка, Git/provenance, source capture, SQL или commit завершились ошибкой, unit откатывается целиком, прежние graph/FTS/files/revision остаются.

«Полный» означает отсутствие stat/hash пропусков для текущих кандидатов и полную логическую замену их валидных проекций, а не физический DROP таблиц. Last-valid строки невалидных файлов — обязательное исключение из замены. Не объявлять физическую corruption recovery реализованной; она принадлежит TK-26.

## Git detection и начальное построение

Проверить реальные checkout/reset/смену ветки, включая удалённые/добавленные пути и changed content. Git-кандидаты и confined наблюдения рабочей копии остаются источником detection; не добавлять HEAD watcher, Git hooks или специальный реестр предыдущей ветки. Обновляющие search/traverse/inspect/check замечают дельту при следующем вызове. `status` остаётся наблюдающим: отсутствующий кэш не создаёт и reindex не запускает.

Удаление только cache.db приводит к первичному полному пути и ожиданию готового graph/FTS до ответа. Не удалять vectors.db/config/Markdown/Git; совпадающие search_hash могут переиспользовать имеющиеся векторы. Из пустого кэша восстановить last-valid проекцию текущего невалидного Markdown невозможно: записывается только его ошибка. Это не обещание восстановления потерянной истории из FTS.

## Атомарность и сигналы

До commit параллельный WAL-reader видит прежнюю опубликованную ревизию, после — новую полную unit. Другой publisher ждёт index.lock и затем повторяет detection. Ошибка/завершение процесса до commit оставляет старый индекс; late signal после commit не откатывает публикацию. Kernel снимает flock после смерти процесса.

Здесь SIGINT проверяет rollback реального процесса по действующей обработке сигнала. Единственный JSON `cancelled` во всех blocking paths и обработанный waiter protocol остаются TK-29; TK-25 их не реализует и не сертифицирует. Тесты отделяют native signal death от обработанного ответа. Не добавлять reindex --cancel, PID cancellation, retries или reindex_state.

## Ответственности и изменения

- `src/service/document_traits.rs`: маленький `Refresher` с полным методом; обновить реализацию test doubles осмысленно, без фиктивного fallback полного метода в refresh.
- `src/service/document_refresher.rs` и его private helper-файлы: lock scope, режим выбора, общий атомарный publication orchestration; сохранить существующие нужные scoped storage generics, не распространять новые.
- `src/service/projection.rs`, `document_refresher/basis.rs`: явный полный разбор/основания и общий last-valid путь; внутренний enum режима допустим по конкретной необходимости, без внешней новой абстракции.
- `src/storage/`: менять только если общему алгоритму нужна узкая реальная операция; ordinary queries — SeaQuery с bound values, без новой схемы и копии Markdown. Contentless FTS last-valid нельзя удалять заранее.
- `src/cli/input.rs`, `handlers.rs`, `output.rs`: transport и вызов Refresher, никаких SQL/Git/file алгоритмов.
- `src/lib.rs`: реальные constructor calls и передача capability в handler; `src/main.rs` остаётся тонким.
- `skills/letopis/SKILL.md` и документация: действующая команда, момент применения, её отличие от incremental refresh и пределы отмены/восстановления.

Соблюдать KISS/DRY/YAGNI, разделение domain/services/technology, top-down reading order, имена-глаголы, meaningful constants, AAA и независимые expected values. Цель около 40 строк, максимум 60 на функцию/метод/тест; split только по реальной ответственности. Не добавлять bundles аргументов конструктора, ненужные wrappers, зависимости или runtime.

## Приёмка и evidence

Проверить реальные DB/CLI сценарии: первый build (включая пустой корпус), force-reparse неизменных файлов, monotonically increasing revision, reindex invalid A + changed B, fresh-cache invalid A, удаления/игнорирование/unmarked/source cleanup, неизменность source_snapshot и Git index/HEAD, reuse vectors, отсутствие inference/wait. Проверить внешнюю Git-смену и следующий incremental вызов, WAL-reader old/new, ожидание publisher с повторным detection, I/O/SQL rollback и реальный SIGINT до commit; late commit остаётся.

Тест полного режима должен независимо доказать повторную вставку FTS/проекций, иначе вызов incremental refresh ошибочно прошёл бы его. Для failure paths проверять исходные revision, nodes, edges, FTS и files, а не только код ошибки. Ручные компилируемые мутации: пропуск force parse, пропуск lock/redetection, подавление rollback/ранний commit, ожидание inference — выбрать точные meaningful controls по изменённому коду, затем byte-for-byte восстановить.

`make check`, затем `make verify` на окончательном дереве. При изменении storage/native/build — новый host/Linux native runner evidence, не переиспользовать исторические receipts как текущие. Для SIGINT/publication сценариев использовать детерминированные checkpoints, без произвольных sleep и без новых production environment hooks. Не запускать платные provider/model прогоны. Платформенные ограничения и непроведённые проверки назвать явно.

Отчёт и машинные результаты сохранить в `docs/zkit/letopis/checks/TK-25/`, новые логи/receipts отдельно в `.artifacts/tk25/`; обновить карточку фактическими результатами. До выполнения критерии остаются unchecked. Merge/push/закрытие issue требуют дальнейшего явного запроса; прежнее разрешение TK-24 не распространяется на TK-25.
