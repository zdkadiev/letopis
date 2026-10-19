# Перенос проекта по структуре zkit 0.2.6

Дата: 2026-10-19. Основание: [D-89](../../decisions.md#D-89). Область — расположение документов, ссылки и пути в проверочных скриптах. Содержание требований/решений, контрактные поля, поведение продукта, зависимости задач и оценки не пересматриваются. Новые замеры не запускались.

## Новое расположение

| Раньше | Теперь |
|---|---|
| resource_model.py | [resource-math/model.py](../../resource-math/model.py) |
| resilience_model.py | [resilience/model.py](../../resilience/model.py) |
| roadmap_schedule.py | [roadmap/schedule.py](../../roadmap/schedule.py) |
| task-template.md | [roadmap/task-template.md](../../roadmap/task-template.md) |
| validation/ | checks/<этап>/; общие материалы в checks/flow/ |
| domain-model.md, scenarios.md, simplification-audit.md, migration.md, migration-manifest.json | appendix/ |
| reference/requirements-pre-zkit.md | appendix/requirements-pre-zkit.md |
| reference/probes/go-sqlite/ | research/R-1-go-sqlite/ |
| reference/probes/rust-sqlite/ | research/R-2-rust-sqlite/ |
| measurements/ollama-2026-10-18/ | research/R-4-ollama-local-probe/ |
| measurements/git-2026-10-18/ | research/R-5-git-storage-probe/ |
| measurements/sqlite-2026-10-18/ | research/R-6-sqlite-capacity-probe/ |
| contracts/x-n-… и i-n-… | contracts/X-n-… и I-n-…; имена схем согласованы со ссылками |
| Историческая подробная часть contracts.md | [contracts/legacy/contracts.md](../../contracts/legacy/contracts.md) |

В корне остаются документы этапов и decisions.md. Разметка Supports у приложений называет поддерживаемый этап. Локальный стандарт карточек сохранён, потому что он описывает согласованные дополнительные поля недельного плана и фактического отчёта; удаление его по общей подсказке плагина потеряло бы этот материал.

Подробная историческая часть contracts.md перенесена целиком, без пересказа или сокращения. В корневом документе остаются текущие таблицы, границы, решения и ссылка на подробности. Приоритет текущих X-1…X-4 и схем сохранён. Поэтому layout checker больше не предупреждает о размере contracts.md.

## Сохранность

Все **240** исходных файлов имеют ровно один адрес назначения; **159** файлов перемещены. В манифесте записаны старые/новые пути и SHA-256 обоих состояний. Тексты сравниваются с исключением изменившихся путей/ссылок, служебных обратных ссылок и явно добавленной информации о миграции. Прежние решения D-1…D-88 и полная историческая часть контрактов сохранены.

Побайтово совпадают **73** архивных файла проб/замеров, модели resource-math/resilience и три mechanics-модели. Исторические JSON/stdout/manifest результатов не переписываются в новый успех. SHA-256-манифесты старых запусков остаются историческими: README могли получить новые ссылки, но исполненные пробы и сырые результаты не изменены. Абсолютные временные пути внутри исполненных проб остаются частью истории запуска; инструкция воспроизведения требует отдельной disposable-копии.

[Недельный план](../../roadmap.md) сохраняет **45 задач, 8 эпиков, 204 / 372 / 672 часа**, expected **372 часа / 38 недель при 10 часах в неделю**. Сохранённый JSON недельного распределения совпадает побайтово, пересчёт новым расположением calculator даёт те же allocations и финальную TK-45. Статусы planned и прежние acceptance/dependencies не меняются.

## Повторные проверки

| Проверка | Результат |
|---|---|
| Новый layout.py | exit 0; OK: layout matches |
| link_docs.py | exit 0; 9 документов этапов, 89 решений, 9 отчётов |
| resource-math, contracts, data, mechanics, resilience, roadmap | Штатные checker-ы exit 0 |
| requirements, architecture — штатный запуск | exit 1: ошибка zkit 0.2.6, IsADirectoryError при чтении каталога research/R-1-go-sqlite |
| requirements, architecture — контрольный запуск | exit 0 после добавления только пропуска каталогов в обходе research/ |
| Недели/карточки/эпики | exit 0; 45/45 карточек, 38 недель, максимум 10 ч/неделю, финальная TK-45 |
| Схемы и примеры внешних контрактов | exit 0; 67 CLI cases, 6 provider examples, 8 metadata cases, 97 assertions |
| Внутренние схемы и прежние модели | exit 0; 69 cases, 3 модели, все 14 отдельных отключений защиты обнаружены |
| Арифметика resilience | exit 0; 17 проверок прежней модели |
| Ссылки и синтаксис | Новых битых ссылок нет; Python-файлы компилируются без исполнения исследовательских проб |

В requirements остаётся прежнее информационное предупреждение о доле Must: 42 из 44, 95%. Приоритеты требований не меняются ради устранения предупреждения. Все результаты и stdout/traceback сохраняются в [checks JSON](layout-migration-0.2.6-checks-2026-10-19.json).

### Ошибка checker-ов zkit 0.2.6

Штатные requirements/architecture scripts используют os.listdir(research) и открывают каждый результат как текстовый файл. По новой схеме рядом с отчётом обязательно лежит каталог его артефактов; на нём происходит IsADirectoryError. Это сбой обхода файлов, а не найденная ошибка требований/архитектуры.

[Контрольный launcher](check_zkit026_compat.py) читает установленный скрипт и добавляет в памяти одну строку:

```python
if not os.path.isfile(os.path.join(rdir, f)): continue
```

Установленный плагин не изменялся, остальные проверки и семантические gates не отключались. Для каждого контрольного запуска отдельно проверено, что реальный дефект документа — дублированный FR/B ID — по-прежнему отклоняется. Оригинальные неуспешные запуски не обозначаются успешными.

## Как повторить

Из корня проекта:

```sh
python3 -B roadmap/schedule.py --check
python3 -B checks/contracts/check_contract_examples.py
python3 -B checks/mechanics/check_mechanics_artifacts.py
python3 -B checks/resilience/check_resilience_arithmetic.py
python3 -B checks/flow/check_zkit026_compat.py . requirements
python3 -B checks/flow/check_zkit026_compat.py . architecture
```

Штатные команды zkit запускаются с этим же корнем. Реальные команды/exit codes и результаты проверки конечного расположения записаны в checks JSON; полный учёт файлов — в [манифесте](layout-migration-0.2.6-manifest-2026-10-19.json).

Проверены документы, схемы, арифметика и ранее созданные state-модели. Не выполнялись новые замеры RAM/latency/диска, inference, сборка исследовательских программ, physical power loss, backup/restore или drills. Draft/not assessed в исходных документах не превращается этой миграцией в подтверждение реализации.
