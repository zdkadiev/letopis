# I-8 / I-9 / I-10 — согласованные чтения
> x-boundary: I-8 · x-direction: internal · x-status: draft
> x-serves: FR-21, FR-22, FR-26, FR-30, FR-32, NFR-7 · x-limits: []

## Layout

```text
IndexReadView { connection: exclusive read-only handle,
  revision: Option<TreeOid>, config: Option<Configuration> }
open_read_view() -> Result<IndexReadView, DomainError>             // I-8
DocumentState { locks: Vec<X1Lock>, errors: Vec<X1Diagnostic> }
IndexState { revision: Option<TreeOid>, embedding_jobs: u64,
  reindex: Option<{stage: X1Stage, completed: Option<Percent>}> }
read_document_state(view: &StateReadView) -> Result<DocumentState, DomainError> // I-9
read_index_state(view: &StateReadView, runtime: RuntimeState) -> Result<IndexState, DomainError> // I-10
```

Configuration — X-1 с max_retries, без credentials; прочитанная конфигурация не подменяется глобальной после model switch. IndexReadView владеет одной SQLite read tx, первое чтение закрепляет revision/config; FTS и vec1 читаются в ней же, закрытие по RAII при успехе/ошибке/отмене. Нельзя использовать соединение параллельно или делать в нём diagnostics write. Один запрос — один read view, не история generations.

StateReadView — та же идея короткого read tx для одного status/errors, без отдельного долговечного состояния. RuntimeState — стадия reindex и completed текущего этапа; процент null, если знаменатель неизвестен, а не ложный 0%. Progress не обещает физической атомарности с CPU шагом. Счётчик queue = COUNT(state='queued'), включает computing/backoff; failed исключает. Errors с active не добавляют скрытый публичный ID, если его нет в X-1.

Пример: A закрепляет S1 и модель M1, config --set публикует M2 и reindex S2. A дочитывает S1/M1; B получает S2/M2. Ни одному запросу не подсовывается vector другой модели. Отдельные файловые чтения агента используют I-12/Git по D-77.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
