# X-3 — Git
> x-boundary: X-3 · x-direction: consumed · x-status: draft
> x-verified: verified (documented object/command subset only, 2026-10-18)
> x-serves: FR-18, FR-24, FR-25, FR-34, C-3, C-8 · x-limits: []

## Layout
Git хранит канонические и исторические байты; letopis сохраняет OID и служебное состояние. snapshot/source_snapshot, обозначающие снимок корпуса/источников, — полный OID tree; revision индекса — не Git OID, а счётчик публикаций ([D-95](../decisions.md#D-95)). SHA-256 содержания letopis не равен Git object ID. В репозитории может быть SHA-1 или SHA-256 object format: передаётся соответствующий полный OID без обрезания. Для source_revisions допускаются существующие Git-ревизии, адресующие содержимое; API передаёт полные OID, не произвольные shell-выражения.

| Операция границы | Вход | Выход / зависимость |
|---|---|---|
| Найти рабочую копию | rev-parse --show-toplevel из каталога вызова | абсолютный корень; вне рабочей копии команда не продолжает работу с произвольным каталогом |
| Проверить снимок | cat-file -e и -t по полному OID | существование и точный тип tree; commit не принимается как source_snapshot только потому, что может быть разыменован |
| Прочитать bytes | cat-file blob <tree_oid>:<path> | исходные bytes blob, без --filters/--textconv; строки вырезаются из этих bytes приложением |
| Перечислить дерево | ls-tree -z, нужная рекурсия задаётся отдельно | mode/type/OID/name с NUL-границами, имена не разбираются по пробелу |
| Записать blob | hash-object -w --no-filters --stdin | OID исходных bytes без clean/EOL-преобразования |
| Построить tree | mktree -z с существующими объектами и непосредственными записями дерева | OID tree, без commit; вложенные деревья подготавливаются отдельно |
| Закрепить дерево | update-ref в служебном refs/letopis namespace | удерживаемый tree, без изменения HEAD/ветки/пользовательского index |

Это требуемые capabilities Git-адаптера; CLI-команды таблицы — независимые семантические ориентиры и историческая реализация TK-8. Уточнение 2026-10-31, issue #67: продукт использует локальную `gix =0.90.0` внутри процесса; runtime Git CLI/helper/filter/network/fetch отсутствует. Порядок физической фиксации и межпроцессное исключение — mechanics. Git CLI нужен для авторских workflows, не для запуска продукта. Строгий malformed-tree oracle зафиксирован на Git 2.50.1, как в host baseline; parity более слабой проверки старых версий Git не обещается. Точный OID не заменяется текущим HEAD. Проверка подтверждённого источника сравнивает адресованные bytes, а не равенство commit OID. Source без текущих ссылок отсутствует в графе: его адрес даёт not_found (D-91).

## Verification boundary — 2026-10-29, implementation update 2026-10-31

`retain` проверяет bytes/type корневого tree и точный direct ref, затем CAS-закрепляет его без обхода дочерних объектов; результат не является `RetainedTree`. Служебный ref обеспечивает достижимость здоровых объектов при обычном GC, не свидетельствует о целостности closure. `pin` каждый раз проверяет полную локальную closure, даже при существующем ref; gitlinks не разыменовываются. Только сильный pin возвращает `RetainedTree`.

Реализация 2026-10-31 сохраняет существующий closure walk и независимо хеширует
raw local objects через Git header/body: collision-detecting SHA-1 или SHA-256.
Структура/mode/name/order деревьев проверяются локально по независимым Git oracles;
прежние cat-file/hash-object subprocess и metadata spool из реализации 2026-10-29
больше не нужны. Текущий object/delta payload может расти с размером объекта;
между объектами освобождается, reusable allocation ограничена 4 MiB, hash chunks
64 KiB; object/delta caches отключены. Repeated supplied blob OIDs проверяются
один раз на assembly, но последующий public call и сильный final pin проверяют
независимо. Direct refs/CAS, no alternates/replace/lazy fetch и safe errors сохранены.
Private refs не создают/дописывают reflogs; существующие logs и user refs не меняются.
Обычные reads проверяют адресованные объекты; полный аудит авторских оснований
выполняется whole-corpus check/reindex до SQL под index.lock. Inspect сильно
закрепляет всё размеченное знание и все доступные объявленные local sources,
включая незапрошенные файлы. Marked source повторно читается из captured blob.
Неизвестный ID отклоняет весь запрос. [Спецификация](../../../superpowers/specs/2026-10-30-issue-67-product-git-optimization-design.md),
[D-105](../decisions.md#D-105).


## Timing and limits
2026-10-19: Согласованное чтение нескольких документов выполняется по одному неизменяемому Git-снимку (существующий snapshot из inspect), [D-77](../decisions.md#D-77). Обычные чтения живых Markdown-файлов разрешены, но могут пересекать правки агентов и не получают межфайловой гарантии. Search/traverse сохраняют собственный согласованный снимок SQLite; он не распространяется автоматически на последующее чтение файлов. Протокол reader locks не вводится; letopis не пишет корпус, способ правки выбирает агент (D-93).

Удерживаются бессрочно только выданные inspect snapshot и source_snapshot, на которые ссылаются знания; снимков before/after на каждую правку нет, letopis не пишет корпус ([D-93](../decisions.md#D-93)). Деревьев на каждую публикацию нет: drain читает текущие файлы ([D-95](../decisions.md#D-95)). Без автоматического коммита; Git отвечает за объекты и достижимость через refs. Служебное дерево содержит нужный корпус и целые доступные файлы адресованных источников, без БД и постороннего кода; агент читает диапазон из целого файла. Никакая SQL-таблица не дублирует bytes. Период упаковки — условие resource-math, не добавленный cron/таймер продукта.

Защищённой записи и восстановления нет ([D-93](../decisions.md#D-93)): изменения через Git — обычные правки файлов, замечаемые следующим вызовом. Обновление Git ref не делает Markdown+SQLite атомарными; наличие refs и exit 0 сами по себе не доказывают сохранность после потери питания. Сохранность закреплённых refs, RPO/RTO и drills остаются mechanics/resilience/P-3. x-limits пуст, потому что resource-math не установил численный L диска/задержки; годовой диск не объявляется проверенным.

## Errors
Неверный формат OID либо неподходящий тип source_snapshot — invalid_request. Синтаксически допустимый отсутствующий OID или неизвестный запрошенный ID — not_found. Если текущие адресованные bytes отличаются от source_snapshot знания, оно получает flag source_changed; отсутствующий или неизвестный снимок — активная диагностика ([D-93](../decisions.md#D-93)). Недоступный источник — source_not_found, без удаления знания. Отсутствующий путь/полный диапазон в существующей ревизии-фильтре означает несовпадение этой пары, не ошибку всего search. Ошибки чтения/записи самого Git или повреждение объекта — internal_error с безопасной диагностикой, а не ложное not_found на любую ошибку object store.

Повтор записи одинакового blob/tree даёт тот же object ID. Обновление ref использует явные предусловия там, где нужен compare-and-swap; блокировка/ошибка не объявляется успехом.

## Verification and sources
2026-10-18, проверка официальных manuals: [rev-parse](https://git-scm.com/docs/git-rev-parse), [cat-file](https://git-scm.com/docs/git-cat-file), [ls-tree](https://git-scm.com/docs/git-ls-tree), [hash-object](https://git-scm.com/docs/git-hash-object), [mktree](https://git-scm.com/docs/git-mktree), [update-ref](https://git-scm.com/docs/git-update-ref), [gc](https://git-scm.com/docs/git-gc), [config](https://git-scm.com/docs/git-config).

Manuals подтверждают форматы и назначение команд, raw/no-filters, точный тип объекта, запись дерева и удержание достижимых refs. Они не подтверждают наш crash-протокол. У update-ref документированы атомарность одной ref и условия транзакции; конкурентный читатель нескольких refs не обязан видеть их как единый снимок. Гарантия NFR-1 не объявляется выполненной. Новых Git-проб, коммитов и измерений для этой проверки не выполнялось.
