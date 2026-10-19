# I-11 — общее хранение диагностики
> x-boundary: I-11 · x-direction: internal · x-status: draft
> x-serves: FR-30, FR-32, FR-33 · x-limits: []

## Layout

```text
Diagnostic { code: X1ErrorCode, message: SafeString,
  details: JsonObject, active: bool, owner: Option<DiagnosticOwner> }
append(unit: &mut WriteUnit, error: Diagnostic) -> InternalRowId
complete(unit: &mut WriteUnit, id: InternalRowId) -> ()
clear_completed(unit: &mut WriteUnit) -> ()
```

InternalRowId — SQLite INTEGER, не пользовательский ID операции. DiagnosticOwner — внутренний enum Reindex либо Embedding(Uuid), кодируемый owner="reindex" / "embedding:<uuid>" в errors; owner не выходит в X-1 JSON. Для active owner обязателен, для завершённых rollback/query/validation diagnostics допускается null. Автор причины выбирает active и owner, storage B-2 не анализирует message. Ссылки error_id в jobs/reindex_state не нужны: владелец находит свою активную диагностику по owner даже после удаления работы или рестарта.

Новая ошибка retry: в той же write unit прежние active того же owner становятся completed, затем добавляется новая active. Предыдущие сообщения не исчезают и остаются до clear; это не агрегация/дедупликация. Успех/замена/удаление embedding-работы завершает active owner данного узла в той же unit. Terminal failed job остаётся со своей active ошибкой; отмена завершает её и добавляет completed cancelled. Истощённый/невалидный reindex удаляет свою строку, но active owner="reindex" остаётся доступен в errors; успешная согласованная публикация исправленного графа завершает его. Отмена новой перестройки не стирает прежнюю неустранённую причину reindex. Обслуживающая подсистема читает/обновляет owner явно, без parsing message или отдельного журнала.

Rollback diagnostic имеет details={hash,paths}; для provider нет ключа, заголовков, полного запроса/патча. Конкретный error catalog — X-1. Если storage unit откатилась, её диагностика тоже не появилась: вызывающий возвращает безопасную ошибку и не утверждает durable запись. Удаление recovery без durable диагностики запрещено.

Errors --clear: DELETE WHERE active=0 в одной tx. Active остаются. Завершение причины и clear сериализуются SQLite; append после clear остаётся. Повторное чтение status/errors ничего не удаляет. Пример: active provider error E1 + completed rollback E2 → clear оставляет только E1; поздний append E3 не удаляется прежним clear.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
