# TK-18 — продуктовый валидатор графа

**Статус:** component ready; все task reviews и final whole-branch review Approved.
Реализация подготовлена в ветке tk-18 от75be0deead9d1317b2fd0f833e2bbc83a526c9b5.
Ревью и исходная верификация выполнялись по снимку рабочей копии до коммита.

## Что реализовано

- domain/validation.rs: deterministic dangling findings и итеративный SCC только
  по includes существующих знаний, без рекурсии и изменения входов.
- service/graph_diagnostics.rs: статeless service по одному caller-owned read view;
  полный graph до owner filter; safe parser reasons, perSCC/file representative,
  typed private cause и отсутствие частичного успеха при storage error.
- DiagnosticKind/get_message/get_code разделяются с Markdown без изменения грамматики.
- lib.rs реально конструирует wire_graph_diagnostics без I/O/дополнительной транзакции.
- Реальная интеграция с DocumentRefresher/Storage проверяет repair lifecycle,
  стабильные ID/created_at при сдвиге строк, invalid-file isolation, pinned snapshot
  при второй публикации и безопасный отказ при намеренной corruption временного cache.

Команды check/status/traverse остаются TK-19/TK-28/TK-23, их приёмка pending.
D-103 разрешает разработку до отложенного G-1, не подтверждая G-1/TK-17/G-2.

## Проверки

- Baseline make check PASS.
- Task1:6focused tests,2semantic controls,makecheckPASS; независимый review Approved.
- Task2:6component tests+28существующих Markdown tests,2semantic controls,makecheckPASS;
  независимый review Approved.
- Task3:5public integration tests PASS;4selected controls restored; review Approved.
  Первый makeverify выявил Clippy ownership в test helper; исправлено заимствованием.
  Итоговый make verify PASS (включает check, strict Rustdoc, root release tests).
- Итого17новых focused scenarios. Это не общий счётчик всех root tests.
- Из8контролей7меняли product logic временно,1был явно test-only fresh-view substitution.
  Все выявлены; byte restoration проверен. Whole-crate mutation sweep не запускался.

Логи .artifacts/tk18/; task briefs/reports/review snapshots —
.superpowers/sdd/2026-10-25-tk-18-graph-validation/.

## Решения исполнения

1. Без автоматических commits: review exact before/after working-tree snapshots,
   включая untracked source, с hashes; index/history не меняются.
2. Constructor перенесён в Task2 с crate-private сервисом из-за deny(warnings).
   Task3 проверяет готовый API без фиктивного начального RED.
3. Разрешён один temporary coordinate control в неизменяемом parser; исходные
   bytes/hash scan.rs восстановлены, product parser grammar не изменена.

## Ограничения

Product storage/native/schema/dependencies не менялись; platform harnesses и
пилоты/RAM/load кампании не запускались. Нет Linux/platform qualification,
публичной CLI check/status/traverse приёмки, G-1/G-2 pass или release claim.
Обычная Rust component реализация и её проверки не заменяют эти обязательства.
Итоговое whole-branch review: Approved,0Critical/0Important/0openMinor.
Одна устаревшая запись статуса в task card исправлена и проверена reviewer.
[Receipt](verification.json) фиксирует source hashes и область верификации.


## Передача

Компонент реализован по утверждённой спецификации; новые17focused tests и финальный
make verify пройдены. CLI check/status, traverse, source/config/embedding status
aggregation, G-1/G-2/RAM/load и platform qualification не объявляются выполненными.
Это независимые задачи/приёмка, а не скрытые пропуски component tests.

2026-10-25: пользователь разрешил commit TK-18, merge в main, push и обновление
GitHub issue с результатами и оставшимися пунктами. История Git фиксирует итоговые
коммиты; [issue #18](https://github.com/zaurkadiev/letopis/issues/18) — передачу и
незавершённые критерии. Worktree и private evidence сохраняются.
