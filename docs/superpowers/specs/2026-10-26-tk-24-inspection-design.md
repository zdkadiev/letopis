# TK-24 — смешанный batch inspect и навык агента

**Состояние:** спецификация по существующей карточке; пользователь выбрал «по карточке», пропустив уточнения. План утверждён командой «go»; пользователь затем явно выбрал Subagent-driven. Реализация Task 1/2 и исправление по Task 2 review проверены; [общий отчёт](../../zkit/letopis/checks/TK-24/report.md) разделяет исторический host checkpoint и новую проверку. Повторный Task 2 review завершён без замечаний. Финальный review ветки на `374c31c` выявил I1: смену bytes ранее неразмеченного Markdown-кандидата при последующем чтении как source. Исправление прошло fresh host runner и replay на сохранённом executable; scoped rereview исправленной ревизии ещё ожидается.

**База:** `main` / `origin/main` = `0f8f92b205f6db48a188cc130c5a6e1190512e3d`, ветка `tk-24`, отдельный managed worktree. Незакоммиченные изменения `tk-23` не переносятся и не изменяются.

## Основания и цель

- [Карточка TK-24](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-24-inspect.md), GitHub #24.
- [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md), [машинная схема](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [I-12 CanonicalSnapshot](../../zkit/letopis/contracts/I-12-snapshot.md), [I-4 источники](../../zkit/letopis/contracts/I-4-sources.md).
- [MC-6 / mechanics](../../zkit/letopis/mechanics.md), [D-77/D-91/D-93/D-95](../../zkit/letopis/decisions.md).
- [Границы программы и их проверка](../../zkit/letopis/architecture.md#program-structure), [AGENTS.md](../../../AGENTS.md).

Завершить уже существующий Inspector: один запрос знаний и источников возвращает актуальные metadata из одного удерживаемого Git tree. Агент затем читает bytes стандартным Git. Дополнить поставляемый `skills/letopis/SKILL.md` действующими сценариями чтения, записи и перепроверки.

Основной пример, выбранный исполнителем по карточке: прочитать знание и связанный диапазон исходника из одного inspect snapshot. Перепроверка после изменения кода также обязательна. Это разработка функциональности, а не новый платный пилот. TK-17 и RAM/G-1/G-2 ограничения не закрываются этой работой.

## Уже есть и что меняется

`InspectionService<D,G,F,M>` обновляет индекс, читает текущую разметку, пишет blobs по одному файлу, создаёт/закрепляет tree. Knowledge-only metadata не берётся из SQL. Для известного source пока возвращается InternalError: это незавершённая TK-24 ветка, которую требуется заменить.

`SourceService::read_source` наблюдает live bytes после SQL membership. Для inspect этот путь не подходит: membership и hash должны происходить из одних захваченных Markdown/bytes, а не из SQL и последующего live чтения. Переиспользуются чистые SourceUri/LocalSource и confined FileSystem/GitSnapshots, без второго source reader, реестра или переноса логики в адаптер.

`ElementLocation::{Local(RepoPath),Remote(SourceUri)}` уже используется traverse. Повторно используем его для `InspectedElement.path`; JSON shape остаётся прежним. Это выбор исполнителя, а не новое требование пользователя.

## Контракт результата

- Вход и наружный интерфейс остаются `Inspector::inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>`.
- Нормализованные повторные ID выдаются однократно в `Inspection.elements`.
- Успех содержит только `snapshot`, `elements` с `path`, необязательными `lines` и `hash`; нет текста, flags, revision, created_at, file_hash или edge ID.
- Knowledge path — путь содержащего файла, lines/hash — точный элемент текущей разметки в возвращаемом tree, по существующим правилам кодека.
- Local source path — путь без `#L…`, lines — объявленный диапазон, либо отсутствуют для целого файла. Hash — SHA-256 адресованных bytes; сохраняются CRLF, LF, binary bytes и отсутствие завершающего LF. Пустой целый файл доступен и хешируется.
- HTTP/HTTPS path — исходный непрозрачный URL вместе с query/fragment. Hash null, lines отсутствуют; ни сеть, ни локальный путь не читаются.
- Source существует только пока хотя бы одна текущая каноническая external-ссылка объявляет его. Старой SQL-строки или наличия файла недостаточно.
- Известный source с отсутствующим файлом либо неполным диапазоном остаётся элементом с `hash: null`. При отсутствии файла пути в tree нет; если файл существует, но диапазон недоступен, целый файл всё равно включается.
- Неизвестный ID, удалённое знание или удалённый якорь отклоняет весь batch с not_found; успешного частичного elements нет. Отсутствующий файл известного source — отдельный случай выше.
- Неверная текущая разметка сохраняет прежнюю безопасную ошибку validation_failed; inspect не подставляет last-valid SQL metadata.
- Тип знания/source по одному адресу согласуется с существующим правилом: точное существующее знание разрешается как знание; отдельной неоднозначной записи с тем же ключом в map нет. Диапазон source остаётся отдельной identity.

## Захват и удержание

1. Выполнить существующий refresh через Refresher.
2. Нормализованные requested ID собрать в set. Перечислить текущие Markdown-кандидаты через CorpusLister, читать/разбирать по одному файлу. Удерживать только metadata, declarations и blob OID, не bytes всего корпуса.
3. Для размеченных файлов записать точные blobs; собрать requested knowledge metadata и все объявленные SourceUri. Перед source I/O отвергнуть неизвестные ID.
4. Создать внутренний unretained corpus tree из уже записанных blobs. Он нужен для повторного использования точных захваченных bytes, если source указывает на файл размеченного корпуса; не перечитывать такой файл из live filesystem.
5. Сгруппировать все объявленные локальные источники по path. На каждый path использовать bytes corpus tree, если файл уже захвачен; иначе одно confined чтение текущего файла. За один проход вычислить metadata всех запрошенных ranges этого path и добавить один blob целого доступного файла. Unmarked Markdown тоже может быть source. Не запрашивать SourceService/SQL для membership.
   Если source path уже наблюдался среди Markdown-кандидатов как неразмеченный или отсутствующий, сохранить только hash первых bytes либо факт отсутствия. Позднее чтение того же path должно совпасть с этим наблюдением; при изменении отклонить весь inspect безопасной typed capture error до pin. Повторная запись идентичных bytes допустима. Это узкая поправка I1 к согласованности metadata/bytes одного path; она не обещает атомарного снимка разных файлов.
6. Собрать конечный tree из всего размеченного корпуса и целых доступных файлов всех объявленных локальных sources, без дублирования path. Посторонние файлы, служебные БД и отсутствующие files не добавляются. Только запрошенные элементы включаются в JSON.
7. Закрепить конечный tree через существующий GitSnapshots::pin до успешного ответа. Внутренний tree не выдаётся и не обязан удерживаться. Ошибка capture/assembly/pin не превращается в успешный результат; безвредные недостижимые Git-объекты допустимы.

Tree фиксирует фактически захваченный набор bytes, а не атомарный межфайловой снимок live working copy. Редактирование между чтениями допускается по D-77/I-12; metadata обязана соответствовать именно bytes в tree. Последующее редактирование или git gc не меняет выданный snapshot. Пользовательский Git index, HEAD и рабочие файлы не изменяются.

Удерживаются списки metadata/paths/OID/references и не более одного обрабатываемого файла bytes. Фиксированный общий RSS или ограничение на размер одного файла здесь не обещается. Новый async runtime, процесс, provider request, storage schema или миграция не нужны.

## Ответственности

- `domain`: существующие SourceUri/LocalSource/Sha256 и правила диапазонов без I/O.
- `service/inspection.rs`: реальное capture orchestration за Inspector с зависимостями только от traits; helper состояния и тесты выделяются по ответственности при необходимости, без пустых новых слоёв.
- `service/command.rs`: metadata-only DTO, `path: ElementLocation`; публичная документация и реальные конструкторы/тесты адаптируются.
- `service/mod.rs`: существующие интерфейсы/экспорты; GitSnapshots расширять не требуется.
- `git`, `filesystem`, `storage`, `embedding`: поведение не расширяется. SourceService остаётся live reader для search/traverse.
- `cli/output.rs`: прежнее сериализуемое `path.as_str()`. Handler вызывает Inspector, не занимается захватом.
- `lib.rs`: существующая фактическая сборка InspectionService сохраняется, её документация меняется с knowledge-only на mixed inspection.

Применяются KISS, DRY, YAGNI; verb names, именованные значимые константы, AAA и независимые expected values, ориентир 40/max 60 строк на функцию. Ненужные generics/dyn/Clone и общая новая snapshot-абстракция не добавляются.

## Навык агента

Обновить существующий skill, сохранив переносимость и root-relative адреса. Агент выбирает каталоги/файлы, собственные relation types по допустимой грамматике; includes/external имеют системную семантику. ID выводятся из path/anchor, не записываются как UUID.

Обязательные сценарии: поиск до записи; формат whole-file/section, ссылки и created_at; check после правок; search/traverse/inspect; чтение смешанного batch через один tree; source_snapshot из существующего полного Git tree OID с проверкой адресованных bytes; разные роли source_snapshot и inspect snapshot; rename/path/anchor и исправление входящих ссылок; dangling/includes-cycle diagnostics; явная перепроверка источников с обновлением основания только после чтения; live reads без межфайловой гарантии.

Для незакоммиченных источников навык может показать временный отдельный Git index и argv-based Git-команды, если проверка подтверждает отсутствие изменений реального index/HEAD. Не требовать commit чужих файлов и не подставлять текущий HEAD как основание без проверки bytes. Не вводить letopis read/snapshot/apply.

Убрать устаревшие claims, что check, source identity, filters/traverse ещё не реализованы. Отдельно оставить незавершённые status/reindex/config/cancel-сценарии TK-25…29 и установку/операции TK-44. Не приписывать TK-29 обработанный cancelled JSON текущему CLI.

## Приёмка и ограничения

Проверить mixed batch, canonical duplicates, unknown/removed element, current hash/lines после правки без доверия SQL, source без ссылок, missing/range-missing/remote/empty/binary/CRLF cases, общую immutable snapshot metadata, все source files в tree, path overlap, pin failure и GC retention. CLI: один schema-valid JSON, ожидаемый exit, без текста/flags и без secret payload.

Тесты поведения должны ловить ручные компилируемые мутации, восстановленные byte-for-byte. Проверки продукта: `make check`, затем финальный `make verify` на изменённом дереве. Платформенные/CLI runner проверки выбираются по реальным изменённым входам; не переобъявлять исторические native receipts текущими. Нативный storage runner обязателен при изменении storage/native/build, которых этот дизайн не требует.

Сохранить specification/plan/report и новые evidence отдельно; результаты и реальные невыполненные проверки отразить в карточке. Платных моделей/пилотов, загрузки HTTP sources, ручной правки БД, публикации main или закрытия issue до явного разрешения нет.
