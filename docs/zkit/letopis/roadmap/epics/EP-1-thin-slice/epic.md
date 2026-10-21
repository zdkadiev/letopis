# EP-1 — Сквозной рабочий slice

> **Фаза:** PH-1
> **Оценка, ч:** 39 / 59 / 104 (best / expected / worst)
> **Зависимости:** [TK-1](../EP-8-proofs/tasks/TK-1-build.md), [TK-2](../EP-8-proofs/tasks/TK-2-durability-harness.md), [TK-3](../EP-8-proofs/tasks/TK-3-vector-harness.md), [TK-4](../EP-8-proofs/tasks/TK-4-runtime-harness.md), [TK-5](../EP-8-proofs/tasks/TK-5-pilot-protocol.md), [TK-6](../EP-8-proofs/tasks/TK-6-provider-harness.md)

## Цель

Реальный CLI сохраняет размеченное знание через Git/SQLite, находит и инспектирует его, вычисляет embedding; подготовлен начальный навык.

## Границы

- **Входит:** Реальный CLI сохраняет размеченное знание через Git/SQLite, находит и инспектирует его, вычисляет embedding; подготовлен начальный навык. Выполняются только задачи, перечисленные ниже.
- **Не входит:** Полные ветви check/source/filter/traverse — EP-2; обслуживание и полный lifecycle — EP-3; target-scale fixture и пилот — EP-7.
- **Интерфейсы:** точные входы/выходы и ссылки на потребителей заданы в карточках. Эпик не вводит отдельный процесс или runtime-сервис.

## Задачи

Разделение следует проверяемым механизмам и артефактам, а не слоям design/code/test. Порядок определяется зависимостями и [недельным планом](../../../roadmap.md); G-1 закрывается последней задачей фазы.

- [TK-7 — Реализовать разметку, ID, хеши и доменные типы](tasks/TK-7-markdown.md): Доменная модель и parser Markdown (без сериализатора: letopis не пишет корпус, D-93), отделённые от Git/SQLite/CLI адаптеров.
- [TK-8 — Реализовать Git snapshots и удержание объектов](tasks/TK-8-git-snapshots.md): Git adapter со снимками, служебными refs и тестами сохранности объектов.
- [TK-9 — Реализовать служебную SQLite и storage ports](tasks/TK-9-storage.md): Начальная схема SQLite и storage adapters для всех согласованных сущностей.
- [TK-10 — Реализовать обнаружение изменений и публикацию по файлу](tasks/TK-10-change-detection.md): Production путь обнаружения изменений и публикации по файлу.
- [TK-11 — Реализовать CLI-процесс, блокировки и JSON](tasks/TK-11-cli-runtime.md): Рабочий CLI без постоянного процесса, с блокировками и запуском drain.
- [TK-12 — Реализовать поиск ссылок и согласованное чтение](tasks/TK-12-search.md): Базовый search/inspect с согласованными ссылками и гибридным ранжированием.
- [TK-13 — Реализовать config и базовый embedding executor](tasks/TK-13-embeddings.md): Базовый embedding runtime и drain по текстам без вектора.
- [TK-14 — Собрать сквозной slice и начальный навык агента](tasks/TK-14-thin-slice.md): Первый работающий сквозной slice и минимальная инструкция coding-агенту.

## Приёмка

Все критерии [TK-7](tasks/TK-7-markdown.md), [TK-8](tasks/TK-8-git-snapshots.md), [TK-9](tasks/TK-9-storage.md), [TK-10](tasks/TK-10-change-detection.md), [TK-11](tasks/TK-11-cli-runtime.md), [TK-12](tasks/TK-12-search.md), [TK-13](tasks/TK-13-embeddings.md), [TK-14](tasks/TK-14-thin-slice.md) выполнены с evidence. Gate фазы отдельно закрывает её последняя задача; промежуточный эпик может передать готовые результаты задачам той же фазы до этого gate. Сам эпик не дублирует выполнение или часы дочерних задач.

## Оценка и риски

Диапазон — сумма карточек, где указаны причины best/expected/worst и действия при неуспехе. Новых ожиданий или резервных часов на уровне контейнера нет. TK-7 выполнена ([отчёт](../../../checks/TK-7/report.md)); TK-8…TK-14 planned. Наличие карточек не является свидетельством выполнения оставшихся задач.
