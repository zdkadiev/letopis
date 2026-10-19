# I-12 — канонический Git-снимок
> x-boundary: I-12 · x-direction: internal · x-status: draft
> x-serves: FR-25, FR-27, FR-28, FR-34 · x-limits: []

## Layout

```text
SnapshotRequest { knowledge_ids: Set<Uuid>, source_ids: Set<Uuid>,
  full_corpus: bool }
CanonicalSnapshot { oid: TreeOid, elements: Vec<NodeMetadata>,
  files: transient Map<Path, {blob_oid: GitOid, file_hash: Sha256}> }
capture(request: SnapshotRequest) -> Result<CanonicalSnapshot, DomainError>
read(snapshot: &CanonicalSnapshot, path: Path) -> ByteStream
harden(snapshot: &CanonicalSnapshot) -> Result<DurableTree, DomainError>
```

Snapshot снимается под project gate после автоматического recovery; один tree охватывает весь размеченный корпус и адресованные внешние source, явно запрошенные source без ссылок тоже включаются. Elements в ответе inspect ограничены запрошенными ID. Существование source/его created_at берётся из долговечного реестра, текущие bytes/hash/lines — из снимка. Отсутствующий source представлен null hash, файл в tree отсутствует. Неизвестный ID отклоняет пакет; read из удерживаемого tree не переключается на live path.

Capture не выполняется из prepare I-2 повторно и не вызывает apply. Reindex получает snapshot один раз; publish проверяет актуальность target/base. Git snapshots, включая каждый принятый source_snapshot и возвращаемый inspect snapshot, отдельно удерживаются refs/letopis/snapshots/<oid>, не пользовательским commit или SQL-копией текста. harden должен гарантировать доступность всех blob/tree/refs после system crash до разрешения journal commit; plain subprocess exit 0 недостаточен. Платформенная реализация/полный flush — P-3, см. R-8/MC-2.

Пример: inspect [U1,U2] → S2, метаданные обоих из S2; последующий apply создаёт S3. read(S2,path1) и read(S2,path2) остаются S2. Обычный open live path может видеть более позднюю/промежуточную версию по D-77. Поля SnapshotRequest/ByteStream только in-process, нового CLI read/snapshot API нет.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
