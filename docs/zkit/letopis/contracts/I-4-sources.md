# I-4 / I-7 — адресованные источники
> x-boundary: I-4 · x-direction: internal · x-status: draft
> x-serves: FR-17, FR-18, FR-19, FR-20, FR-23, FR-24 · x-limits: []

## Реализованная граница TK-20 — 2026-10-25

[Утверждённая спецификация](../../../superpowers/specs/2026-10-25-tk-20-source-identity-design.md):
`SourceReader::read_source(&mut StorageRead, &SourceUri)` возвращает
`Result<Option<SourceObservation>, DocumentError>`. Caller владеет уже открытым
pinned read view и refresh. Reader не открывает БД, не публикует и не обновляет граф.
Отсутствующий source или knowledge по этому адресу даёт `None` до I/O.
Локальный source наблюдается через confined FileSystem: точные bytes файла либо
включительного диапазона, без UTF-8/EOL преобразования. Результат содержит только
метаданные и `Available` либо `SourceNotFound`; недоступный файл/полный диапазон
имеет `hash=None`, unsafe/I/O/storage failures остаются типизированной ошибкой.

HTTP/HTTPS — непрозрачный исходный адрес с query/fragment: `hash=None`, `lines=None`,
`Unfetched`, без filesystem/network I/O. Это неизвестные bytes, не утверждение 404.
`SourceNotFound` — типизированная причина для будущих CLI consumers; новых публичных
полей/flags TK-20 не добавляет. Live bytes наблюдаются заново при следующем чтении,
а принадлежность и `created_at` берутся из pinned view. Source text не возвращается
и не хранится. Старые кэши требуют [явной пересборки](../../../development.md#tk-20-явная-пересборка-кэша-при-обновлении).

Следующий Layout задаёт целевой исторический контракт: `validate_basis`, `observe`,
`source_changed` и сравнение Git bytes остаются TK-21; TK-20 их реализацию не заявляет.

## Layout

```text
SourceAddress { uri: NormalizedUri, lines: Option<InclusiveLines> }
SourceObservation { address: SourceAddress, hash: Option<Sha256> }
BasisInput { knowledge: KnowledgeId, source_snapshot: TreeOid,
  sources: Set<SourceAddress>, current: WorkingCopy }
validate_basis(input: BasisInput) -> Result<(), DomainError>      // I-4
observe(knowledge: NodeMetadata, indexed: IndexReadView,
  current: WorkingCopy, filters: X1Filters)
  -> Result<{flags: Set<X1Flag>, matches: bool}, DomainError>     // I-7
```

WorkingCopy — текущие файлы рабочей копии, прочитанные в этом вызове; основания читаются Git-объектами по source_snapshot. CanonicalSnapshot (I-12) используется только inspect ([D-95](../decisions.md#D-95)).

InclusiveLines: start/end целые, 1≤start≤end, обе включительны. Без диапазона читаются bytes всего файла. Validate_basis не отказывает индексации [D-93](../decisions.md#D-93): он сравнивает bytes по всем связям знания в одном основании; несовпадение даёт флаг source_changed, недоступный tree или диапазон — диагностику file:<путь знания> и флаг перепроверки. Неверный тип OID — invalid_request, отсутствующий существующий по форме объект — not_found; повреждение Git — internal_error, не маскируется.

Observe не подтверждает знание и не пишет новый source_snapshot. Flags из существующего X-1, совпадение source+revision проверяется на одной привязке; исторический hash вычисляется из Git, не берётся из текущего source hash. Без обнаруженной причины flags пуст; это не проверка истинности. Чтение одинакового path внутри вызова можно разделить временным владением, постоянного source cache нет. Веб-загрузчик в порт не входит.

Вход-пример: source_snapshot=S1, source=src/a.rs#L2-L5; current — рабочая копия. Равные адресованные bytes дают matches=true для фильтра S1 даже при другом commit, неравные — source_changed. Исчезнувший диапазон даёт source_not_found и сохранение знания; новая ссылка на него индексируется с этим флагом и диагностикой. Выход только метаданные/состояние, не исходный текст.

При индексации validate_basis передаёт каждый найденный в метаданных source_snapshot Git-порту для pin в существующем refs namespace ([D-93](../decisions.md#D-93)); отсутствующий tree — диагностика и признак перепроверки, не отказ. Ссылочная достижимость basis обязательна независимо от совпадения bytes.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit.
