# TK-7 — Реализовать разметку, ID, хеши и доменные типы

> **Статус:** done
> **Эпик:** [EP-1](../epic.md)
> **Фаза:** PH-1
> **Оценка, ч:** 5 / 8 / 14 (best / expected / worst)
> **План:** неделя 5 — 4 ч; неделя 6 — 4 ч
> **Зависимости:** [TK-1](../../EP-8-proofs/tasks/TK-1-build.md), [TK-2](../../EP-8-proofs/tasks/TK-2-durability-harness.md), [TK-3](../../EP-8-proofs/tasks/TK-3-vector-harness.md), [TK-4](../../EP-8-proofs/tasks/TK-4-runtime-harness.md), [TK-5](../../EP-8-proofs/tasks/TK-5-pilot-protocol.md), [TK-6](../../EP-8-proofs/tasks/TK-6-provider-harness.md)


## 1. Цель

Сквозной путь должен соединить реальные адаптеры; ранние RAM и usefulness результаты нужны до расширения полного функционала. Конкретный результат этой задачи: доменная модель и parser Markdown (без сериализатора: letopis не пишет корпус, D-93), отделённые от Git/SQLite/CLI адаптеров.

## 2. Контекст и входные данные

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-1, B-2, X-4.
- Покрывает: [FR-3, FR-4, FR-5, FR-6, FR-35, C-7](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-4](../../../../contracts/X-4-markdown.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-0. Текущий G-1 закрывается в TK-17 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Создать доменные типы и разбор актуальной грамматики Markdown X-4 на `pulldown-cmark` (`into_offset_iter`, `ENABLE_HEADING_ATTRIBUTES`, D-96).
2. Реализовать файл-элементы и раздел-элементы с якорями, ID по пути/якорю, created_at, рёбра-тройки/includes и source URI (D-93).
3. Добавить fixtures точных байтов/хешей, fenced code, вложенности, дублей якорей и ошибок разметки.

## 4. Ожидаемый результат

Доменная модель и parser Markdown (без сериализатора: letopis не пишет корпус, D-93), отделённые от Git/SQLite/CLI адаптеров.

## 5. Критерии приёмки

- [x] Markdown codec распознаёт элементы и title X-4 (`<!-- letopis: {...} -->`, `{#anchor}`, `letopis:type=…`), ID по пути/якорю, ссылки и source URI; метавершина выводится по исходящим includes, поля kind нет (D-95).
- [x] Точные байты/SHA-256 и created_at проходят fixtures.
- [x] domain отделён от адаптеров.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

## 7. Границы

- **Входит:** Доменная модель и parser Markdown (без сериализатора: letopis не пишет корпус, D-93), отделённые от Git/SQLite/CLI адаптеров. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Git adapter — [TK-8](TK-8-git-snapshots.md); SQL projection — [TK-9](TK-9-storage.md); полный итоговый graph validator — [TK-18](../../EP-2-knowledge/tasks/TK-18-graph-validation.md).
- **Интерфейсы — вход:** [TK-1](../../EP-8-proofs/tasks/TK-1-build.md): Воспроизводимый build harness и отчёт о совместимости выбранной связки на двух ОС; [TK-2](../../EP-8-proofs/tasks/TK-2-durability-harness.md): Матрица прерываний публикации с точным oracle и воспроизводимый harness обнаружения/публикации; [TK-3](../../EP-8-proofs/tasks/TK-3-vector-harness.md): Регрессионные сценарии vec1/read view и пропуска устаревшего текста по search_hash; [TK-4](../../EP-8-proofs/tasks/TK-4-runtime-harness.md): Проверенная координация процессов: одна публикация, один drain, отмена SIGINT с negative fixtures; [TK-5](../../EP-8-proofs/tasks/TK-5-pilot-protocol.md): Исполнимый протокол P-1 и формы отчёта для раннего и финального пилота; [TK-6](../../EP-8-proofs/tasks/TK-6-provider-harness.md): Boundary harness двух API с воспроизводимыми негативными transport-сценариями
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-8](TK-8-git-snapshots.md), [TK-9](TK-9-storage.md), [TK-10](TK-10-change-detection.md), [TK-14](TK-14-thin-slice.md), [TK-18](../../EP-2-knowledge/tasks/TK-18-graph-validation.md).

## 8. Риски и примечания

**Основание оценки 5 / 8 / 14 ч:** best — fixtures X-4 однозначно задают грамматику; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — неоднозначные Markdown/code-block границы и точное сохранение байтов. Это инженерный диапазон, не измеренная скорость.

**Риск и действие:** Неоднозначные Markdown/code-block границы и точное сохранение байтов. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## 9. Отчёт о выполнении

Выполнена 2026-10-21 в отдельной ветке TK-7 от main@48b8ed8. Создан корневой Rust library crate letopis: domain в src/domain, read-only parser X-4 и единый search renderer в src/adapters/markdown. Доменные типы не зависят от Markdown/JSON/Git/SQL/HTTP/CLI; parser принимает path+bytes и не выполняет I/O. Возвращаются ID, точные byte/line ranges и raw SHA-256, created_at/source_snapshot, рёбра-тройки и временный search text/hash; никаких serializer/UUID/kind-полей.

[Согласованная спецификация](../../../../../../superpowers/specs/2026-10-21-tk-7-markdown-domain-design.md), [выполненный план](../../../../../../superpowers/plans/2026-10-21-tk-7-markdown-domain.md), [отчёт](../../../../checks/TK-7/report.md), [верификация](../../../../checks/TK-7/verification.json), [ревью](../../../../checks/TK-7/review.md). Коммиты реализации: 0c32513 (domain), 524231f (parser), 79cc52d (search/evidence), 3aff95f (context fixes).

Все три критерия подтверждены: 30 integration tests (6 domain, 10 elements, 6 links, 8 search), fmt и clippy PASS на macOS ARM64 и Linux glibc ARM64; Linux runtime без сети. Точные байты/хеши, LF/CRLF/Unicode/EOF, metadata/errors, fenced/indented/HTML context, anchors, нормализация адресов, links и search invariants проверены воспроизводимыми fixtures. Один свежий reviewer нашёл 2 Important, оба подтверждены RED→GREEN и исправлены, suite повторён на обеих платформах; Critical/отложенных Minor нет.

Git adapter/SQL projection/CLI/полный graph validator и реальные FS/symlink/source bytes остаются следующим задачам, согласно границам выше. Ресурсные цели и x86_64 не объявляются проверенными. Оценки/расписание не менялись. Приёмка завершена локально; merge/push/изменение GitHub issue выполняются отдельно по запросу пользователя.
