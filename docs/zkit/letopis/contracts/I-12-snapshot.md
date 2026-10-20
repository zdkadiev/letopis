# I-12 — канонический Git-снимок
> x-boundary: I-12 · x-direction: internal · x-status: draft
> x-serves: FR-25, FR-27, FR-28, FR-34 · x-limits: []

## Layout

```text
SnapshotRequest { element_ids: Set<Uuid>, full_corpus: bool }
InspectedElement { path: Path, lines: Option<InclusiveLines>,
  hash: Option<Sha256>, file_hash: Option<Sha256> }
CanonicalSnapshot { oid: TreeOid, elements: Map<Uuid, InspectedElement>,
  files: transient Map<Path, {blob_oid: GitOid, file_hash: Sha256}> }
capture(request: SnapshotRequest) -> Result<CanonicalSnapshot, DomainError>
read(snapshot: &CanonicalSnapshot, path: Path) -> ByteStream
harden(snapshot: &CanonicalSnapshot) -> Result<DurableTree, DomainError>
```

Snapshot снимается под project gate после автоматического recovery; один tree охватывает весь размеченный корпус и адресованные внешние source, явно запрошенные source без ссылок тоже включаются. Element_ids адресуют вершины, метавершины, source и рёбра; повторные ID обрабатываются один раз. Elements содержит общие InspectedElement только запрошенных ID, без зависимости от NodeMetadata проекций. Полный tree/manifest остаётся входом prepare; NodeMetadata/EdgeMetadata строит кодек из его bytes, а не из ограниченного результата inspect. Существование source/его created_at берётся из долговечного реестра, текущие bytes/hash/lines — из снимка. Отсутствующий source представлен null hash, файл в tree отсутствует. Неизвестный ID отклоняет пакет; read из удерживаемого tree не переключается на live path.

Ребро разрешается по его UUID в канонической ссылке размеченного источника в этом же tree. Path — путь источника; lines — включительный диапазон самой Markdown-ссылки, не всей вершины; hash = SHA-256(JCS({source, markdown})), где source — UUID вершины-источника, markdown — точные bytes ссылки как UTF-8 строка. File_hash общий для всех элементов того же файла. Устаревшие SQL hash/lines не подставляются. Если ребра уже нет в текущем tree, это not_found всей группы даже при старой строке в индексе. Пример смешанной группы inspect [N,E,S] возвращает метаданные узла N, ссылки E и зарегистрированного source S из одного снимка; неизвестный ID не даёт частичного ответа.

Capture не выполняется из prepare I-2 повторно и не вызывает apply. Reindex получает snapshot один раз; publish проверяет актуальность target/base. Git snapshots, включая каждый принятый source_snapshot и возвращаемый inspect snapshot, отдельно удерживаются refs/letopis/snapshots/<oid>, не пользовательским commit или SQL-копией текста. harden должен гарантировать доступность всех blob/tree/refs после system crash до разрешения journal commit; plain subprocess exit 0 недостаточен. Платформенная реализация/полный flush — P-3, см. R-8/MC-2.

Пример: inspect [U1,U2] → S2, метаданные обоих из S2; последующий apply создаёт S3. read(S2,path1) и read(S2,path2) остаются S2. Обычный open live path может видеть более позднюю/промежуточную версию по D-77. Поля SnapshotRequest/ByteStream только in-process, нового CLI read/snapshot API нет.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
