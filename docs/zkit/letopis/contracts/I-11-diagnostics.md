# I-11 — текущие проблемы
> x-boundary: I-11 · x-direction: internal · x-status: draft
> x-serves: FR-5, FR-11, FR-30, FR-32, FR-33 · x-limits: []

## Layout

```text
Problem { code: X1ErrorCode, message: SafeString, details: JsonObject }
current_problems(view: &StateReadView, config: ConfigCheck) -> Result<Vec<Problem>, DomainError>
check_problems(view: &StateReadView, paths: Option<Set<Path>>) -> Result<Vec<Problem>, DomainError>
```

Диагностика — только текущие проблемы, выводимые из данных ([D-95](../decisions.md#D-95)); отдельной таблицы ошибок, истории, active/owner и очистки нет. Источники проблем: строка files с текстом ошибки (невалидный файл; пишется в той же unit публикации, что и проекции, MC-2), запрос рёбер без существующей цели (висячая ссылка), обход includes-подграфа (цикл), отсутствующие в Git source_snapshot (поиск различных значений из метаданных при status/check), строки embed_failures в vectors.db с исчерпанными попытками, строка query_error(at, error) в vectors.db — последняя ошибка embedding запроса до следующего успешного, проверка `.letopis/config.json` при вызове. Исправленная причина исчезает из ответа без отдельного действия.

details проблемы файла — {path, причина, target при висячей ссылке}; проблемы embeddings — {element_id или search_hash, безопасное сообщение}; для provider нет ключа, заголовков, полного запроса. Ошибка embedding поискового запроса записывается в query_error (одна строка, перезаписывается следующей ошибкой и удаляется успешным embedding запроса); search при этом отвечает через FTS (D-62). Сбой запуска drain — предупреждение (warnings) в ответе команды, не сохраняется. Конкретный error catalog — X-1.

Удаление кэша не теряет проблем: следующая индексация заново записывает files и находит висячие ссылки и циклы. Удаление vectors.db или смена конфигурации сбрасывают попытки и их ошибки (FR-30). Пример: файл A невалиден, B ссылается на удалённый C → status.problems = [A: validation_failed, B→C: висячая ссылка]; после исправления A и ссылки в B список пуст.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
