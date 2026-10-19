# Проверки миграции

Результаты штатных zkit scripts сохраняются здесь. Прохождение миграции означает сохранность содержания и связей, не выполнение ещё открытых design gates. Не редактировать вывод checker для имитации успешной проверки.


## Структура zkit 0.2.6

Результаты распределены по этапам в checks/<step>/. Исторические JSON/stdout не переписываются как свежие проверки. Текущая проверка переноса: [отчёт](flow/layout-migration-0.2.6-2026-10-19.md) и [результаты](flow/layout-migration-0.2.6-checks-2026-10-19.json).

Ошибка обхода research/ в локальных requirements/architecture checker-ах исправлена 2026-10-20. Текущие штатные проверки проходят напрямую, без launcher; [результаты исправления](flow/research-directory-fix-2026-10-20.md). Прежний [контрольный launcher](flow/check_zkit026_compat.py) сохранён только как материал исторической диагностики; старые traceback и результаты контрольных прогонов не переписывались.

Недельный план проверяется из корня проекта командой `python3 -B roadmap/schedule.py --check`. Схемы/примеры: `python3 -B checks/contracts/check_contract_examples.py`; модели: `python3 -B checks/mechanics/check_mechanics_artifacts.py`; арифметика восстановления: `python3 -B checks/resilience/check_resilience_arithmetic.py`. Это проверки документов и прежних моделей, не новые замеры.
