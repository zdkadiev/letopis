# I-2 — подготовка и публикация проекций
> x-boundary: I-2 · x-direction: internal · x-status: draft
> x-serves: FR-13, FR-27, FR-28, FR-29 · x-limits: []

## Layout

Внутрипроцессный Rust-порт, не wire API. Все методы возвращают Result<T, DomainError>; DomainError имеет code из X-1, безопасные message и details. OID — полный существующий Git tree, UUID/hash — типы X-1; Paths — проверенные root-relative пути. Текст существует только во временных буферах.

```text
ProjectionInput { snapshot: TreeOid, base_revision: Option<TreeOid>,
  changed_paths: Set<Path>, changed_ids: Set<Uuid>, full: bool,
  config_identity: Option<Sha256> }
PreparedProjection { snapshot: TreeOid, base_revision: Option<TreeOid>,
  config_identity: Option<Sha256>,
  nodes: Vec<NodeMetadata>, edges: Vec<EdgeMetadata>, removed_ids: Set<Uuid>,
  search: Vec<{id: Uuid, uri: NormalizedUri, search_hash: Sha256}> }
prepare(input: ProjectionInput, snapshot: CanonicalSnapshot) -> PreparedProjection
publish(unit: &mut WriteUnit, prepared: PreparedProjection) -> ()
```

NodeMetadata — id/kind/uri/hash/created_at/source_snapshot/line_start/line_end/search_hash из storage-schema.sql, row_id сохраняется по UPSERT. EdgeMetadata — id/source_id/target_id/type/hash/created_at. Для отсутствующего source hash=null, source_snapshot=null, search_hash=null. Search хранит только адреса/хеши; prepare не удерживает Vec текстов полного корпуса. При publish immutable snapshot читается последовательно, одно собственное представление MC-1 за раз вставляется в FTS и освобождается. Повторного capture/I-12 нет: read обращается к уже закреплённому tree через Git-порт. Это не загрузка содержимого включённых знаний. Полный reindex может удерживать одну write unit дольше обычного delta apply; его atomic publication не превращается в обещание короткой фиксированной задержки. Removed IDs не включают долговечные source без ссылок.

Подготовка не меняет БД и не получает новый snapshot через I-12. Publish присоединяется к переданной SQLite unit; не делает BEGIN/COMMIT, callback к редактору или модели. Caller проверяет канонический target/base в project gate. Apply публикует также recovery deletion; reindex — reindex row deletion. Изменённые nodes/edges/FTS, несовместимые vectors, project revision и jobs согласованы атомарно. Никакой новый operation ID/история ответов не создаётся.

Вход-пример: snapshot=S2, base_revision=S1, changed_ids={U1}, full=false. Выход: подготовленная строка U1 и собственный search text; после publish+caller COMMIT revision=S2 и queued job с новым seq, либо прежний vector при том же search_hash. При ошибке caller откатывает unit; канонические файлы восстанавливает MC-2. Все S/U здесь символы типов, не валидные wire OID/UUID.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
