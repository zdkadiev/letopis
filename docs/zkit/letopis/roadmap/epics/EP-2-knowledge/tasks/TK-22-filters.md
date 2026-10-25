# TK-22 — Завершить поисковые filters и fallback

> **Статус:** done — implementation verified; Task 1–3 reviews passed; whole-branch review found no product/architecture defects. Its actual verdict was With fixes solely for the metadata inconsistency reconciled here; scoped metadata re-review of commit `7ce1753` is complete: the finding is addressed and no new breakage was found.
> **Эпик:** [EP-2](../epic.md)
> **Фаза:** PH-2
> **Оценка, ч:** 4 / 8 / 16 (best / expected / worst)
> **План:** неделя 16 — 3 ч; неделя 17 — 5 ч
> **Зависимости:** [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md), [TK-20](TK-20-source-identity.md), [TK-21](TK-21-provenance.md)


## 1. Цель

Нужно довести принятые контракты и инварианты до полного v1 scope, сохранив общий путь сохранения/чтения. Конкретный результат этой задачи: полные поисковые фильтры и проверенная деградация query embedding.

## 2. Контекст и входные данные

- **Структура программы и границы модулей:** [обязательные архитектурные правила](../../../../architecture.md#program-structure), включая [порядок проверки](../../../../architecture.md#program-structure-review).

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-6, A-AR5, X-1.
- Покрывает: [FR-20, FR-21, FR-22, FR-23, FR-24](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-1](../../../../contracts/X-1-cli.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-1. Текущий G-2 закрывается в TK-29 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Реализовать комбинации filters с AND между полями и OR внутри списков.
2. Сопоставлять source/revision в одной привязке и фильтровать до итогового limit.
3. Проверить пустые списки, неизвестную revision и FTS fallback без query embedding warning в ответе search: безопасная singleton query_error показывается только через status по D-62; совместно с TK-28.

## 4. Ожидаемый результат

Полные поисковые фильтры и проверенная деградация query embedding.

## 5. Критерии приёмки

- [x] Filters сочетаются AND, списки OR, пустые/отсутствующие списки равны.
- [x] source/revision совпадают в одной привязке.
- [x] Фильтрация до итогового limit, неизвестная revision даёт not_found, query failure не превращает обычную FTS выдачу в ошибку; storage/executor lifecycle проверен, показ status/problems остаётся TK-28.

- [x] Соблюдены [структура программы и границы модулей](../../../../architecture.md#program-structure); в отчёте названы проверенные границы/файлы и evidence либо обосновано отсутствие изменений продуктового Rust-кода.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

**Архитектурная проверка:** выполнить [общий порядок проверки структуры](../../../../architecture.md#program-structure-review) для изменяемых компонентов и зафиксировать результат в отчёте; область без изменений продуктового Rust-кода указать явно.

## 7. Границы

- **Входит:** Полные поисковые фильтры и проверенная деградация query embedding. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Базовый rank — [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md); source matching — [TK-21](TK-21-provenance.md); общий status/error storage — [TK-28](../../EP-3-maintenance/tasks/TK-28-diagnostics.md).
- **Интерфейсы — вход:** [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md): Базовый search/inspect с согласованными ссылками и гибридным ранжированием; [TK-20](TK-20-source-identity.md): Source identity по нормализованному URI, выводимая из ссылок (D-91), и корректные metadata доступности; [TK-21](TK-21-provenance.md): Проверка происхождения и оснований знания с корректными freshness flags
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-24](TK-24-inspect.md), [TK-35](../../EP-4-system-validation/tasks/TK-35-search-latency.md).

## 8. Риски и примечания

**Основание оценки 4 / 8 / 16 ч:** best — готовые read/source порты позволяют прямые fixtures фильтров; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — комбинации AND/OR/source+revision и наблюдаемая деградация query embedding. Это инженерный диапазон, не измеренная скорость.

**Риск и действие:** Комбинации AND/OR/source+revision и наблюдаемая деградация query embedding. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## Уточнение согласованности 2026-10-23

При подготовке TK-12 исправлена устаревшая формулировка пункта 3: [D-62](../../../../decisions.md#D-62), X-1 и MC-6 уже требуют обычный FTS success без query warning; ошибка видна через status. Это исправление карточки по принятому решению, не новое поведение или выполненная приёмка. Полные filters и source freshness остаются за TK-21/TK-22.

## 9. Отчёт о выполнении

Реализация проверена 2026-10-25: [отчёт](../../../../checks/TK-22/report.md),
[machine-readable receipt](../../../../checks/TK-22/verification.json), product `bee627e19956da317fa45961ab6d4321eb0741d4`.
Typed filters/strict CLI, same-binding historical comparison, complete eligible ranking,
filtered FTS fallback и same-identity query_error lifecycle реализованы без новой schema/runtime.
Late error/read cleanup и concurrent publication подтверждены controlled fixtures и runtime
mutations с точным восстановлением. `make verify V=1`: debug/release по
620 passed, 0 failed, 5 ignored; strict Rustdoc/Clippy прошли.
Архитектурная self-review и все 10 spec items/5 Review Focus cases перечислены в отчёте.
Task 1 и Task 2 reviews прошли; Task 3 review и scoped evidence re-review прошли. Whole-branch review на `f099224` не нашёл product/architecture defects и вынес фактический verdict **With fixes** только из-за несогласованного metadata, исправленного здесь. Scoped re-review metadata commit `7ce1753` завершён: finding ADDRESSED, новых проблем нет. Task завершён. TK-28 UI,
Linux/native harness rerun, реальные providers/deadlines, corpus/pilot/resource/gates не заявляются.
