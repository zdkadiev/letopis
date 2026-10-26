# TK-23 — Реализовать полный traverse

> **Статус:** done
> **Эпик:** [EP-2](../epic.md)
> **Фаза:** PH-2
> **Оценка, ч:** 6 / 8 / 16 (best / expected / worst)
> **План:** неделя 17 — 5 ч; неделя 18 — 3 ч
> **Зависимости:** [TK-18](TK-18-graph-validation.md), [TK-20](TK-20-source-identity.md), [TK-21](TK-21-provenance.md), [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md)


## 1. Цель

Нужно довести принятые контракты и инварианты до полного v1 scope, сохранив общий путь сохранения/чтения. Конкретный результат этой задачи: полный traverse CLI со ссылками на знания и опциональными пройденными рёбрами.

## 2. Контекст и входные данные

- **Структура программы и границы модулей:** [обязательные архитектурные правила](../../../../architecture.md#program-structure), включая [порядок проверки](../../../../architecture.md#program-structure-review).

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-6, X-1.
- Покрывает: [FR-5, FR-21, FR-26, NFR-7](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-1](../../../../contracts/X-1-cli.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-1. Текущий G-2 закрывается в TK-29 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Реализовать BFS обход relations/containment/both и направления.
2. Добавить согласованные depth/node/edge limits, dedup и includes как один шаг.
3. Проверить исключение стартов, возврат только пройденных edges и закреплённый read view.

## 4. Ожидаемый результат

Полный traverse CLI со ссылками на знания и опциональными пройденными рёбрами.

## 5. Критерии приёмки

- [x] Relations/containment/both, incoming/outgoing/both, типы/depth/node/edge limits проходят fixtures.
- [x] includes=один шаг, старты исключены, edges только реально пройденные, dedup/cycles конечны, текст не возвращается.

- [x] Соблюдены [структура программы и границы модулей](../../../../architecture.md#program-structure); в отчёте названы проверенные границы/файлы и evidence либо обосновано отсутствие изменений продуктового Rust-кода.

- [x] Обязательный Linux arm64 native/standalone runner завершился полным PASS на741930e; исходный Docker FAIL сохранён исторически.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

**Архитектурная проверка:** выполнить [общий порядок проверки структуры](../../../../architecture.md#program-structure-review) для изменяемых компонентов и зафиксировать результат в отчёте; область без изменений продуктового Rust-кода указать явно.

## 7. Границы

- **Входит:** Полный traverse CLI со ссылками на знания и опциональными пройденными рёбрами. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Хранение/валидация includes — [TK-9](../../EP-1-thin-slice/tasks/TK-9-storage.md)/[TK-18](TK-18-graph-validation.md); search ranking — [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md); batch metadata — [TK-24](TK-24-inspect.md).
- **Интерфейсы — вход:** [TK-18](TK-18-graph-validation.md): Полный доменный валидатор графа с ошибками по файлу; [TK-20](TK-20-source-identity.md): Source identity по нормализованному URI, выводимая из ссылок (D-91), и корректные metadata доступности; [TK-21](TK-21-provenance.md): Проверка происхождения и оснований знания с корректными freshness flags; [TK-12](../../EP-1-thin-slice/tasks/TK-12-search.md): Базовый search/inspect с согласованными ссылками и гибридным ранжированием
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-24](TK-24-inspect.md).

## 8. Риски и примечания

**Основание оценки 6 / 8 / 16 ч:** best — BFS использует готовый закреплённый граф; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — граничные depth/node/edge limits и дедупликация при combined traversal. Это инженерный диапазон, не измеренная скорость.

**Риск и действие:** Граничные depth/node/edge limits и дедупликация при combined traversal. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## 9. Отчёт о выполнении

Реализованы полный metadata-only traverse CLI, инкрементальный BFS в GraphService через GraphRead, чистый domain Graph, закреплённый read view и постраничные source observations. Все продуктовые критерии и архитектурные границы подтверждены; Task1–4 reviews, I1 rereview и final whole-branch review с M1/M2 corrections завершены без открытых замечаний.

[Отчёт TK-23](../../../../checks/TK-23/report.md) и [verification.json](../../../../checks/TK-23/verification.json) фиксируют host native PASS на `e9a9cd4a4c9d6807b0a75d79022dc8f838796e78`, supplemental revised-test verification на `5859e3ea15929b57abc6aafdb3dc477241c87f2d` и новый Linux arm64 native/standalone PASS на `741930ed871e60a0987d655cf79defdff67defd3`. Debug/release отдельно684/0/5 в48root executables; Linux schema parity,10Python tests, real60sdeadline,nativeSQLite3.53.2/vec1 0.8, standalone/linkage прошли. Все202скопированных в контейнер входа совпадают с текущим319-file outer map. Runtime network none;2test threads; owned container cleanup выполнен. Исходный FAIL после read-only Docker и исходные hashes сохранены, не переименованы в PASS.

Все20шагов плана завершены. BFS bound не означает fixed RSS всей команды; публикация/file/root/JSON имеют отдельные расходы. Cache-v2 rebuild остаётся явной quiescent cache-only процедурой с сохранением vectors/config/locks/Markdown/Git. Handled SIGINT/cancelled JSON/once-guard остаются TK-29. TK-24/TK-28, paid/model/pilot/resource/G-1/G-2 и exhaustive cancellation drills не сертифицированы. Merge/push/GitHub publication не выполнялись; интеграция требует отдельного решения пользователя.
