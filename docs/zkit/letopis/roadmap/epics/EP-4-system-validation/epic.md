# EP-4 — Полная системная проверка

> **Фаза:** PH-3
> **Оценка, ч:** 38 / 68 / 122 (best / expected / worst)
> **Зависимости:** [TK-6](../EP-8-proofs/tasks/TK-6-provider-harness.md), [TK-13](../EP-1-thin-slice/tasks/TK-13-embeddings.md), [TK-15](../EP-7-early-validation/tasks/TK-15-target-corpus.md), [TK-16](../EP-7-early-validation/tasks/TK-16-early-memory.md), [TK-21](../EP-2-knowledge/tasks/TK-21-provenance.md), [TK-22](../EP-2-knowledge/tasks/TK-22-filters.md), [TK-25](../EP-3-maintenance/tasks/TK-25-reindexing.md), [TK-26](../EP-3-maintenance/tasks/TK-26-corruption.md), [TK-27](../EP-3-maintenance/tasks/TK-27-embedding-lifecycle.md), [TK-28](../EP-3-maintenance/tasks/TK-28-diagnostics.md), [TK-29](../EP-3-maintenance/tasks/TK-29-cancellation.md), [TK-33](../EP-5-final-pilot/tasks/TK-33-pilot-analysis.md)

## Цель

Полная реализация получает ресурсные измерения и DR-1…DR-8 с явным исходом G-3.

## Границы

- **Входит:** Полная реализация получает ресурсные измерения и DR-1…DR-8 с явным исходом G-3. Выполняются только задачи, перечисленные ниже.
- **Не входит:** Разработка основных механизмов — EP-1/EP-2/EP-3; сравнительный пилот — EP-5; упаковка и приёмка поставки — EP-6.
- **Интерфейсы:** точные входы/выходы и ссылки на потребителей заданы в карточках. Эпик не вводит отдельный процесс или runtime-сервис.

## Задачи

Разделение следует проверяемым механизмам и артефактам, а не слоям design/code/test. Порядок определяется зависимостями и [недельным планом](../../../roadmap.md); G-3 закрывается последней задачей фазы.

- [TK-34 — Проверить полный RAM и десять клиентов](tasks/TK-34-mixed-memory.md): Полная проверка NFR-2/NFR-4/NFR-7 с реальной mixed-client нагрузкой.
- [TK-35 — Оценить полный search и холодный запуск](tasks/TK-35-search-latency.md): Latency отчёт полного поиска и решение по NFR-3 без обрезания результата по времени.
- [TK-36 — Оценить годовой диск и Git обслуживание](tasks/TK-36-disk-growth.md): Обоснованная оценка годового диска и стоимости штатного Git обслуживания.
- [TK-37 — Испытать полный ввод и гонки embedding queue](tasks/TK-37-queue-drill.md): Полный queue/reader/cancellation отчёт P-2.4 и DR-6.
- [TK-38 — Провести crash и физические power-loss drills](tasks/TK-38-power-loss.md): Физические свидетельства DR-1: прерванная публикация не оставляет смешанного кэша.
- [TK-39 — Испытать corruption, Git-смену и freshness](tasks/TK-39-corruption-drill.md): Corruption/freshness отчёт DR-3/DR-4 и доказательство координированного C-8 сценария.
- [TK-40 — Проверить внешний backup и restore на чистом устройстве](tasks/TK-40-restore.md): Воспроизводимый внешний backup/restore drill Git/Markdown и `.letopis/config.json` с пересборкой кэша (D-91); служебного состояния.
- [TK-41 — Испытать providers, секреты и полностью offline режим](tasks/TK-41-provider-drill.md): Provider/offline/security отчёт DR-5 и проверка NFR-6 на обеих платформах.
- [TK-42 — Испытать storage exhaustion и границы доверия](tasks/TK-42-exhaustion.md): Exhaustion/trust-boundary отчёт DR-7/DR-8 и решение о допуске поставки.

## Приёмка

Все критерии [TK-34](tasks/TK-34-mixed-memory.md), [TK-35](tasks/TK-35-search-latency.md), [TK-36](tasks/TK-36-disk-growth.md), [TK-37](tasks/TK-37-queue-drill.md), [TK-38](tasks/TK-38-power-loss.md), [TK-39](tasks/TK-39-corruption-drill.md), [TK-40](tasks/TK-40-restore.md), [TK-41](tasks/TK-41-provider-drill.md), [TK-42](tasks/TK-42-exhaustion.md) выполнены с evidence. Gate фазы отдельно закрывает её последняя задача; промежуточный эпик может передать готовые результаты задачам той же фазы до этого gate. Сам эпик не дублирует выполнение или часы дочерних задач.

## Оценка и риски

Диапазон — сумма карточек, где указаны причины best/expected/worst и действия при неуспехе. Новых ожиданий или резервных часов на уровне контейнера нет. Все задачи planned: наличие карточек не является свидетельством выполнения.
