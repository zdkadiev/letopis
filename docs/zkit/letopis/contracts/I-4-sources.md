# I-4 / I-7 — адресованные источники
> x-boundary: I-4 · x-direction: internal · x-status: draft
> x-serves: FR-17, FR-18, FR-19, FR-20, FR-23, FR-24 · x-limits: []

## Layout

```text
SourceAddress { uri: NormalizedUri, lines: Option<InclusiveLines> }
SourceObservation { address: SourceAddress, hash: Option<Sha256> }
BasisInput { knowledge: KnowledgeId, source_snapshot: TreeOid,
  sources: Set<SourceAddress>, current: CanonicalSnapshot }
validate_basis(input: BasisInput) -> Result<(), DomainError>      // I-4
observe(knowledge: NodeMetadata, indexed: IndexReadView,
  current: CanonicalSnapshot, filters: X1Filters)
  -> Result<{flags: Set<X1Flag>, matches: bool}, DomainError>     // I-7
```

InclusiveLines: start/end целые, 1≤start≤end, обе включительны. Без диапазона читаются bytes всего файла. Validate_basis не отказывает индексации [D-93](../decisions.md#D-93): он сравнивает bytes по всем связям знания в одном основании; несовпадение даёт флаг source_changed, недоступный tree или диапазон — диагностику file:<путь знания> и флаг перепроверки. Неверный тип OID — invalid_request, отсутствующий существующий по форме объект — not_found; повреждение Git — internal_error, не маскируется.

Observe не подтверждает знание и не пишет новый source_snapshot. Flags из существующего X-1, совпадение source+revision проверяется на одной привязке; исторический hash вычисляется из Git, не берётся из текущего source hash. Без обнаруженной причины flags пуст; это не проверка истинности. Чтение одинакового path внутри вызова можно разделить временным владением, постоянного source cache нет. Веб-загрузчик в порт не входит.

Вход-пример: source_snapshot=S1, source=src/a.rs#L2-L5; current=S2. Равные адресованные bytes дают matches=true для фильтра S1 даже при другом commit, неравные — source_changed. Исчезнувший диапазон даёт source_not_found и сохранение знания; новая ссылка на него индексируется с этим флагом и диагностикой. Выход только метаданные/состояние, не исходный текст.

При индексации validate_basis передаёт каждый найденный в метаданных source_snapshot Git-порту для pin в существующем refs namespace ([D-93](../decisions.md#D-93)); отсутствующий tree — диагностика и признак перепроверки, не отказ. Ссылочная достижимость basis обязательна независимо от совпадения bytes.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
