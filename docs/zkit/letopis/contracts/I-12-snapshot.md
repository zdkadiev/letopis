# I-12 — канонический Git-снимок
> x-boundary: I-12 · x-direction: internal · x-status: draft
> x-serves: FR-25, FR-34 · x-limits: []

## Layout

```text
SnapshotRequest { element_ids: Set<ElementId> }
InspectedElement { path: Path, lines: Option<InclusiveLines>,
  hash: Option<Sha256> }
CanonicalSnapshot { oid: TreeOid, elements: Map<ElementId, InspectedElement>,
  files: transient Map<Path, {blob_oid: GitOid}> }
capture(request: SnapshotRequest) -> Result<CanonicalSnapshot, DomainError>
read(snapshot: &CanonicalSnapshot, path: Path) -> ByteStream
pin(snapshot: &CanonicalSnapshot) -> Result<RetainedTree, DomainError>
```

Snapshot снимается с текущих файлов рабочей копии ([D-93](../decisions.md#D-93)); правка агента, идущая в этот момент, может попасть в снимок частично, как при любом чтении живых файлов; один tree охватывает весь размеченный корпус и адресованные внешние source. Source без текущих ссылок не существует (D-91): его адрес — неизвестный ID. Element_ids адресуют вершины, метавершины и source; повторные ID обрабатываются один раз. Elements содержит общие InspectedElement только запрошенных ID, без зависимости от NodeMetadata проекций. Существование source и его created_at (минимум created_at его external-ссылок) выводятся из канонических ссылок этого же tree, текущие bytes/hash/lines — из снимка; реестра source в БД нет, D-91. Отсутствующий source представлен null hash, файл в tree отсутствует. Неизвестный ID отклоняет пакет; read из удерживаемого tree не переключается на live path.

Рёбра inspect не адресует: ребро — тройка (источник, цель, тип) без собственного ID и хеша, его видно через traverse ([D-93](../decisions.md#D-93)). Для знания path/lines — диапазон элемента в этом tree, hash — SHA-256 его байтов; для source — адресованные байты или null. Устаревшие SQL hash/lines не подставляются. Если элемента уже нет в текущем tree, это not_found всей группы даже при старой строке в индексе. Пример смешанной группы inspect [N, S] возвращает метаданные знания N и source S из одного снимка; неизвестный ID не даёт частичного ответа.

Capture используется только inspect; публикация и reindex читают файлы рабочей копии и снимков не создают ([D-95](../decisions.md#D-95)). Git snapshots, включая каждый принятый source_snapshot и возвращаемый inspect snapshot, отдельно удерживаются refs/letopis/snapshots/<oid>, не пользовательским commit или SQL-копией текста. pin должен закрепить ref до выдачи snapshot агенту; журнала записи больше нет ([D-93](../decisions.md#D-93)). Платформенная сохранность refs — P-3, см. R-8.

Пример: inspect [N1,N2] → S2, метаданные обоих из S2; последующая правка агента даёт S3. read(S2,path1) и read(S2,path2) остаются S2. Обычный open live path может видеть более позднюю/промежуточную версию по D-77. Поля SnapshotRequest/ByteStream только in-process, нового CLI read/snapshot API нет.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit.
