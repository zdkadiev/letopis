# Letopis — закрытие четырёх замечаний аудита

Дата: 2026-10-20. Основание: [D-90](../../decisions.md#D-90) и прямое поручение пользователя исправить все четыре finding. [Исходный аудит](full-audit-2026-10-20.md) и его evidence сохранены без изменения. Это исправления спецификации и конечных моделей; продуктовый код не создавался.

## Итог

**A-1…A-4 закрыты на уровне дизайна.** Независимое ревью не нашло оставшихся P1/P2 в исправлениях. Все 13 штатных/проектных проверок проходят в реальном репозитории; при откате только моделей/схемы к исходному commit новые regression cases выявляют прежние разрывы. Готовность реализации, физическая durability и ресурсные цели по-прежнему не заявляются.

| Finding | Исправление | Проверка и будущий владелец |
|---|---|---|
| A-1, P1 | [MC-2](../../mechanics.md) / [I-2](../../contracts/I-2-projections.md): publication delta = published revision → after tree; включает непубликованные внешние изменения/создания/удаления. Request paths остаются областью файловой записи и rollback. Без отставания применяется только обычный delta. | [model_commit.py](../../mechanics/model_commit.py): все сочетания внешних B/create/delete и commit/rollback, isolated external_delta guard. Интеграция — TK-2/TK-10/TK-25/TK-39. |
| A-2, P2 | [X-1](../../contracts/X-1-cli.md) / [I-11](../../contracts/I-11-diagnostics.md): StoredDiagnostic требует boolean details.active из existing errors.active. В DB details второй копии нет; immediate Failure не изменён. Status/errors сохраняют обе группы до clear, query errors остаются видимыми. | Положительные/отрицательные schema cases для status/errors/errors --clear, model diagnostic_state guard; TK-28 и DR-4/DR-6. |
| A-3, P2 | [I-12](../../contracts/I-12-snapshot.md): generic element_ids и общие InspectedElement path/lines/hash/file_hash. Рёбра разрешаются в канонической ссылке того же tree; собственный JCS-хеш и координаты ссылки не берутся из устаревшего SQL. Unknown отклоняет весь batch. | Mixed node+edge пример и model edge_inspection guard: edge-only, mixed source/node/edge, повтор ID и unknown; TK-24. Дополнительные публичные поля/команды не вводятся. |
| A-4, P2 | [Resource-math](../../resource-math.md): backlog и populated состояния; populated требует 100k current vectors по config/search_hash, actual A-2 byte profile и counts queued/failed. FTS fallback не засчитывается как populated semantic run. | Усилены TK-15/TK-16/TK-34/TK-35. Это критерии будущих проверок, новые замеры сейчас не выполнялись; срока опустошения очереди нет. |

Уточнения внесены также в requirements/data/architecture/resilience, подробные доменный/storage материалы и roadmap. Девять исходных карточек, названных в плане исправления, плюс существующая TK-39 как владелец DR-4 получили точные regression acceptance. Новых эпиков, задач, процессов, SQL-таблиц/колонок или provider-параметров нет.

## Свежие проверки

Команды, stdout/stderr, red/green evidence и результаты независимого ревью сохранены в [checks JSON](audit-fixes-checks-2026-10-20.json). Использован существующий Python с PyYAML/jsonschema/referencing; schema validation не пропущена.

| Проверка | Результат |
|---|---|
| requirements/resource-math/contracts/data/architecture/mechanics/resilience/roadmap | 8/8 exit 0 |
| Layout | exit 0, zkit 0.2.6 |
| Weekly schedule | exit 0, 45 задач, 8 эпиков, 372 ч / 38 недель, dependency/gate errors 0 |
| Внешние схемы/примеры | exit 0: 86 CLI cases, 6 provider examples, 8 metadata cases, 116 assertions |
| Внутренние схемы/модели | exit 0: 88 cases, 3 normal models без нарушений; все 17 isolated guard removals выявлены |
| Resilience arithmetic | exit 0: 17 checks |
| Red regressions на исходных моделях/схеме | Ожидаемый exit 1: 12 внешних и 15 внутренних ошибок, включая все три отсутствующих model guards; не считаются провалом исправленной версии |
| link_docs.py на изолированной копии | exit 0: 9 stage docs, 90 decisions, 9 reports; ни одного изменения |
| Локальные ссылки / Python syntax | Новых missing targets нет; 18 Python-файлов разобраны. Три старых фиктивных inline-target в архивах не являются навигационными ссылками |
| Mermaid syntax | 9/9 blocks/files разобраны; без визуального рендеринга |
| Исторический недельный JSON / research | Побайтно сохранены; новые benchmarks/provider calls 0 |
| git diff --check | exit 0 |

Предупреждение requirements о 95% Must (42/44) сохраняется как объяснённое свойство перенесённого согласованного scope; ради него требования не ослаблялись. Исторические отчёты со старыми counts и результатами не переписывались задним числом.

## Границы

Все 45 execution tasks остаются planned, оценки 204/372/672 часа и недельные allocations не менялись. Source URI escaping и воспроизводимое окружение проверок остаются отдельными низкоприоритетными заметками исходного аудита; это поручение закрывает A-1…A-4. Ресурсные/physical/provider испытания выполняются только в соответствующих будущих задачах. Изменения не закоммичены и не отправлены на GitHub этим поручением.
