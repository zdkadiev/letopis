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

Configuration — X-1 с max_retries, без credentials; прочитанная конфигурация не подменяется глобальной после model switch. IndexReadView владеет одной SQLite read tx, первое чтение закрепляет revision кэша и meta.config_identity vectors.db (в кэше identity нет, [D-95](../decisions.md#D-95)); конфигурация читается и синхронизируется через [условный сброс MC-3](../mechanics.md#mc-3--embeddings-работа-из-данных-и-попытки) до открытия read view: нужный сброс выполняется отдельной транзакцией vectors.db без index.lock/embed.lock. После закрепления read view захваченная конфигурация принимается только при совпадении её SHA-256 канонического JSON (JCS) с закреплённой config_identity (D-91); расхождение даёт FTS-only в этом запросе без сброса через read-соединение или повторного открытия view; FTS и vec1 читаются в ней же, закрытие по RAII при успехе/ошибке/отмене. Нельзя использовать соединение параллельно или писать через него. Конфликт предварительного наблюдения даёт FTS-only без provider-запроса и повторного reset/pin; Missing/Invalid/Changed не мешают graph traversal. Условный сброс не устанавливает атомарность с внешними правками config-файла. Один запрос — один read view, не история generations.

StateReadView — та же идея короткого read tx для одного status, без отдельного долговечного состояния. embedding_backlog = число опубликованных search_hash без вектора и без исчерпанных попыток, embedding_failed = число с исчерпанными попытками (embed_failures в vectors.db, тот же read tx); прогресса reindex нет ([D-95](../decisions.md#D-95)). Problems вычисляются I-11 в том же read tx; locks — занятые flock с PID.

Пример: A закрепляет revision 6 и модель M1; пользователь меняет `.letopis/config.json` на M2, следующий вызов сбрасывает только vectors.db (revision кэша — счётчик публикаций graph/FTS — от сброса не растёт и остаётся 6). A дочитывает 6/M1; B получает 6/M2 и до пересчёта векторов ищет по FTS. Ни одному запросу не подсовывается vector другой модели. Отдельные файловые чтения агента используют I-12/Git по D-77.

## Уточнение TK-12 — 2026-10-23

Порядок readiness/reset → pin → сравнение identity закреплён утверждённой [спецификацией TK-12](../../../superpowers/specs/2026-10-23-tk-12-search-design.md) и [D-102](../decisions.md#D-102). Query embedding использует захваченные config/dimensions; HTTP не удерживает write unit. Короткая guarded-запись query_error использует отдельное соединение, не обновление read view. Это согласование I-8 с MC-6, не утверждение уже реализованного пути.

## Проверка search TK-22 — 2026-10-25

[Отчёт](../checks/TK-22/report.md) проверяет effective filters при concurrent
publication: metadata, source bindings и basis берутся из исходного owned read view,
а eligibility считается до channel ranks/fusion/limit. Поздняя provenance/comparison
ошибка после успешного query возвращает safe error без partial links и освобождает
reader до drop живого SearchService; последующие WAL checkpoint и publication проходят.
Query_error очищается фактическим следующим успешным query под той же config identity,
без config reset; отдельная guarded diagnostic-запись не делает read view writable.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit.

## Проверка traverse TK-23 — 2026-10-26

Refresh → configuration readiness/reset → один scoped read view: GraphService читает компактные identities и bounded neighbor pages, TraversalService гидратирует выбранные metadata и наблюдает outgoing external bindings из того же view. Источники передаются provenance по одной странице до получения следующей, независимо от traversal filters. Нет query inference, повторного pin или записи через read handle; Missing/Invalid config не мешают обходу.

[Регрессии TK-23](../checks/TK-23/report.md) проверяют публикацию другим SQLite writer после первой страницы, сохранение исходных membership/coordinates/basis/revision и удалённого последнего source reference. Late page/source/provenance errors, успех и depth-zero empty result освобождают reader, пока сервис жив: настоящий WAL TRUNCATE checkpoint и последующая публикация проходят. Live files сохраняют прежнее отсутствие межфайловой атомарности. Host runner PASS; новый Linux arm64 native/standalone runner PASS на741930e, исходный read-only Docker FAIL сохранён исторически. Task4 independent review и I1 rereview пройдены; final whole-branch review завершён: Critical/Important product findings нет, code quality approved with M1/M2 documentation corrections; M1/M2 documentation corrections approved by scoped re-review; no open code or documentation finding.
