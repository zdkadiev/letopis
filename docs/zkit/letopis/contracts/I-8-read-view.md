# I-8 / I-9 / I-10 — согласованные чтения
> x-boundary: I-8 · x-direction: internal · x-status: draft
> x-serves: FR-21, FR-22, FR-26, FR-30, FR-32, NFR-7 · x-limits: []

## Layout

```text
IndexReadView { connection: exclusive read-only handle,
  revision: Option<u64>, config_identity: Option<Sha256>,
  vectors_meta: Option<VectorsMeta>, config: Option<Configuration> }
open_read_view() -> Result<IndexReadView, DomainError>             // I-8
DocumentState { locks: Vec<X1Lock>, problems: Vec<I11Problem> }
IndexState { revision: Option<u64>, embedding_backlog: u64, embedding_failed: u64 }
read_document_state(view: &StateReadView) -> Result<DocumentState, DomainError> // I-9
read_index_state(view: &StateReadView) -> Result<IndexState, DomainError> // I-10
```

Configuration — X-1 с max_retries, без credentials; прочитанная конфигурация не подменяется глобальной после model switch. IndexReadView владеет одной SQLite read tx, первое чтение закрепляет revision кэша и meta.config_identity vectors.db (в кэше identity нет, [D-95](../decisions.md#D-95)); сама конфигурация читается из `.letopis/config.json` и принимается, только если её SHA-256 канонического JSON (JCS) равен config_identity (D-91), иначе вызов выполняет сброс MC-3 одной транзакцией vectors.db без блокировок; FTS и vec1 читаются в ней же, закрытие по RAII при успехе/ошибке/отмене. Нельзя использовать соединение параллельно или делать в нём diagnostics write. Один запрос — один read view, не история generations.

StateReadView — та же идея короткого read tx для одного status, без отдельного долговечного состояния. embedding_backlog = число опубликованных search_hash без вектора и без исчерпанных попыток, embedding_failed = число с исчерпанными попытками (embed_failures в vectors.db, тот же read tx); прогресса reindex нет ([D-95](../decisions.md#D-95)). Problems вычисляются I-11 в том же read tx; locks — занятые flock с PID.

Пример: A закрепляет revision 6 и модель M1; пользователь меняет `.letopis/config.json` на M2, следующий вызов сбрасывает векторы и публикует revision 7. A дочитывает 6/M1; B получает 7/M2. Ни одному запросу не подсовывается vector другой модели. Отдельные файловые чтения агента используют I-12/Git по D-77.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
