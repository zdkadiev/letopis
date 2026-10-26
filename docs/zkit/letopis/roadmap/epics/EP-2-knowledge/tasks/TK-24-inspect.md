# TK-24 — Завершить batch inspect и навык чтения/записи

> **Статус:** I1 fixed and fresh host verification PASS; scoped rereview pending
> **Эпик:** [EP-2](../epic.md)
> **Фаза:** PH-2
> **Оценка, ч:** 4 / 6 / 10 (best / expected / worst)
> **План:** неделя 18 — 6 ч
> **Зависимости:** [TK-19](TK-19-check-renames.md), [TK-20](TK-20-source-identity.md), [TK-21](TK-21-provenance.md), [TK-22](TK-22-filters.md), [TK-23](TK-23-traversal.md)


## 1. Цель

Нужно довести принятые контракты и инварианты до полного v1 scope, сохранив общий путь сохранения/чтения. Конкретный результат этой задачи: полный batch inspect и рабочий навык чтения/записи перед G-2.

## 2. Контекст и входные данные

- **Структура программы и границы модулей:** [обязательные архитектурные правила](../../../../architecture.md#program-structure), включая [порядок проверки](../../../../architecture.md#program-structure-review).

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-4/MC-6, [D-77](../../../../decisions.md#D-77), X-1.
- Покрывает: [FR-1, FR-2, FR-25, FR-36, C-1](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-1](../../../../contracts/X-1-cli.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-1. Текущий G-2 закрывается в TK-29 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Завершить batch inspect по одному общему Git-снимку.
2. Проверить metadata/lines/hash и отказ всего batch при неизвестном ID; flags относятся к search/traverse, не к inspect.
3. Расширить навык полными сценариями source_snapshot, переименования/висячих ссылок/перепроверки и live reads.

Для element_ids знаний и source реализовать InspectedElement из I-12 (path/lines/hash, общий snapshot); рёбра inspect не адресует, они видны через traverse (D-93). Не читать устаревшие hash/координаты из SQL вместо текущей разметки.

## 4. Ожидаемый результат

Полный batch inspect и рабочий навык чтения/записи перед G-2.

## 5. Критерии приёмки

- [x] Пакет ID читается по общему Git-снимку, unknown ID отклоняет весь запрос.
- [x] Актуальные собственные хеши/lines проверены; дополнительных полей flags в inspect нет.
- [x] Навык покрывает формат элемента (путь/якорь, метаданные, ссылки), check, search/traverse/inspect, source_snapshot средствами Git, переименование/висячие ссылки/перепроверку и обычные live reads; поздние reindex/config/status/cancel интегрируются в TK-29.

- [x] Смешанный batch знаний и source, повторные ID и исчезнувший в текущем tree элемент проверены: дубли однократны, unknown или исчезнувший ID отклоняет весь batch.

- [x] Соблюдены [структура программы и границы модулей](../../../../architecture.md#program-structure); в отчёте названы проверенные границы/файлы и evidence либо обосновано отсутствие изменений продуктового Rust-кода.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

**Архитектурная проверка:** выполнить [общий порядок проверки структуры](../../../../architecture.md#program-structure-review) для изменяемых компонентов и зафиксировать результат в отчёте; область без изменений продуктового Rust-кода указать явно.

## 7. Границы

- **Входит:** Полный batch inspect и рабочий навык чтения/записи перед G-2. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Reindex/status/cancel и полный навык этих команд — [TK-25](../../EP-3-maintenance/tasks/TK-25-reindexing.md)…[TK-29](../../EP-3-maintenance/tasks/TK-29-cancellation.md); установка/операционные инструкции — [TK-44](../../EP-6-delivery/tasks/TK-44-operations.md).
- **Интерфейсы — вход:** [TK-19](TK-19-check-renames.md): Полная проверка корпуса и обработка удалений/переименований согласованного CLI; [TK-20](TK-20-source-identity.md): Source identity по нормализованному URI, выводимая из ссылок (D-91), и корректные metadata доступности; [TK-21](TK-21-provenance.md): Проверка происхождения и оснований знания с корректными freshness flags; [TK-22](TK-22-filters.md): Полные поисковые фильтры и проверенная деградация query embedding; [TK-23](TK-23-traversal.md): Полный traverse CLI со ссылками на знания и опциональными пройденными рёбрами
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-25](../../EP-3-maintenance/tasks/TK-25-reindexing.md), [TK-29](../../EP-3-maintenance/tasks/TK-29-cancellation.md), [TK-30](../../EP-5-final-pilot/tasks/TK-30-pilot-setup.md), [TK-44](../../EP-6-delivery/tasks/TK-44-operations.md).

## 8. Риски и примечания

**Основание оценки 4 / 6 / 10 ч (пересчёт 2026-10-20):** после D-93 inspect не адресует рёбра и не возвращает file_hash, в навыке нет conflict-сценариев; best — текущий snapshot и read/source порты уже поддерживают batch; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — согласование актуальных metadata/lines и примеров read/check при исчезнувшем source. Это инженерный диапазон, не измеренная скорость.

**Риск и действие:** Согласование актуальных metadata/lines и примеров read/check при исчезнувшем source. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## 9. Отчёт о выполнении

Реализация: `bb6b39e9ef5a60d2f65e3da2ec16d97a2c742865` (Inspector, Task 1), `094c174accfe16d246645d780cd7717b3862dc9d` (CLI acceptance, portable skill, Task 2), `5801e6b86227520e76008e71dfdd2c879de857a3` (workflow fix по review). Финальный review на `374c31c` выявил I1 same-path race; исправление удерживает hash/absence первого наблюдения Markdown-кандидата и отклоняет изменённый source-read до pin. Чистый Task 2 rereview и M1 status correction отражены в текущем handoff. Общий долговечный отчёт и evidence index: [TK-24 report](../../../../checks/TK-24/report.md) и [verification.json](../../../../checks/TK-24/verification.json). Локальные task/review заметки под `.superpowers/sdd/2026-10-26-tk-24-inspection/` сохранены как история исполнения.

Исторический host checkpoint `tests/cli/run.py --platform host` — PASS на своей зафиксированной input map: по 697 root tests passed, 0 failed, 5 ignored в debug и release. После I1 fix свежий host runner — PASS на новой карте 205 inputs, встроенный `make verify V=1` exit 0: по 702 root tests passed, 0 failed, 5 ignored в debug и release; вложенные child-helper вызовы посчитаны отдельно. Новый сохранённый executable SHA-256 `11782a47c81d7058204b98996ffcdfc8d32b79fc531b640aea2c1d1014dc8359`. Replay исходной гонки на нём: unchanged control exit 0; unmarked→marked и unmarked→invalid exit 1 с `internal_error`, без partial result, с неизменными index/HEAD. Текущий disposable workflow на том же executable — PASS с точными `source_changed` и `dangling_target`. Evidence: `.artifacts/tk24/host-20261026-final-fix1/`, `.artifacts/tk24/final-fix1/`; прежние receipts сохранены отдельно. Python collector tests: 20 passed на прежнем Task 2 checkpoint, повторно для I1 fix не запускались.

Task 2 не менял `src/`; I1 fix меняет только `service/inspection.rs` и его private tests, сохраняя `Inspector` interface, CLI handler/output, domain и `lib.rs` composition root. Linux, native storage runner, реальная модель/пилот, handled cancellation, G-1/G-2 не объявлены проверенными. Native runner не требовался: storage/native/build входы не менялись. Scoped rereview исправленной ревизии и запись в Engram выполняет контроллер; до этого статус остаётся review pending, без merge/push/GitHub closure.
