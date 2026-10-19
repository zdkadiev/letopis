# EP-2 — Полные знания, источники и чтение

> **Фаза:** PH-2
> **Оценка, ч:** 32 / 56 / 96 (best / expected / worst)
> **Зависимости:** [TK-7](../EP-1-thin-slice/tasks/TK-7-markdown.md), [TK-8](../EP-1-thin-slice/tasks/TK-8-git-snapshots.md), [TK-9](../EP-1-thin-slice/tasks/TK-9-storage.md), [TK-10](../EP-1-thin-slice/tasks/TK-10-save-recovery.md), [TK-12](../EP-1-thin-slice/tasks/TK-12-search.md), [TK-17](../EP-7-early-validation/tasks/TK-17-early-pilot.md)

## Цель

Полные правила графа, CRUD, source identity/provenance и read-контракты проверены на fixture-наборах и доступны агенту.

## Границы

- **Входит:** Полные правила графа, CRUD, source identity/provenance и read-контракты проверены на fixture-наборах и доступны агенту. Выполняются только задачи, перечисленные ниже.
- **Не входит:** Обязательная durable атомарность уже принадлежит TK-10 в EP-1; reindex/jobs/status/cancel и full CLI интеграция — EP-3.
- **Интерфейсы:** точные входы/выходы и ссылки на потребителей заданы в карточках. Эпик не вводит отдельный процесс или runtime-сервис.

## Задачи

Разделение следует проверяемым механизмам и артефактам, а не слоям design/code/test. Порядок определяется зависимостями и [недельным планом](../../../roadmap.md); G-2 закрывается последней задачей фазы.

- [TK-18 — Завершить graph validation и временные ID](tasks/TK-18-graph-validation.md): Полный доменный валидатор графа и atomic validation failures.
- [TK-19 — Завершить edit/delete/перенос и optimistic conflicts](tasks/TK-19-mutations.md): Полные CRUD/перенос и optimistic conflict semantics согласованного CLI.
- [TK-20 — Завершить source identity, URI и доступность](tasks/TK-20-source-identity.md): Долговечный реестр source identity и корректные metadata доступности.
- [TK-21 — Завершить source_snapshot и проверку происхождения](tasks/TK-21-provenance.md): Проверка происхождения и оснований знания с корректными freshness flags.
- [TK-22 — Завершить поисковые filters и fallback](tasks/TK-22-filters.md): Полные поисковые фильтры и проверенная деградация query embedding.
- [TK-23 — Реализовать полный traverse](tasks/TK-23-traversal.md): Полный traverse CLI со ссылками на знания и опциональными пройденными рёбрами.
- [TK-24 — Завершить batch inspect и навык чтения/записи](tasks/TK-24-inspect.md): Полный batch inspect и рабочий навык чтения/записи перед G-2.

## Приёмка

Все критерии [TK-18](tasks/TK-18-graph-validation.md), [TK-19](tasks/TK-19-mutations.md), [TK-20](tasks/TK-20-source-identity.md), [TK-21](tasks/TK-21-provenance.md), [TK-22](tasks/TK-22-filters.md), [TK-23](tasks/TK-23-traversal.md), [TK-24](tasks/TK-24-inspect.md) выполнены с evidence. Gate фазы отдельно закрывает её последняя задача; промежуточный эпик может передать готовые результаты задачам той же фазы до этого gate. Сам эпик не дублирует выполнение или часы дочерних задач.

## Оценка и риски

Диапазон — сумма карточек, где указаны причины best/expected/worst и действия при неуспехе. Новых ожиданий или резервных часов на уровне контейнера нет. Все задачи planned: наличие карточек не является свидетельством выполнения.
