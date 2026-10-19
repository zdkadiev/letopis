# I-4 / I-7 — адресованные источники
> x-boundary: I-4 · x-direction: internal · x-status: draft
> x-serves: FR-17, FR-18, FR-19, FR-20, FR-23, FR-24 · x-limits: []

## Layout

```text
SourceAddress { uri: NormalizedUri, lines: Option<InclusiveLines> }
SourceObservation { address: SourceAddress, hash: Option<Sha256> }
BasisInput { knowledge: Uuid, source_snapshot: TreeOid,
  sources: Set<SourceAddress>, current: CanonicalSnapshot }
validate_basis(input: BasisInput) -> Result<(), DomainError>      // I-4
observe(knowledge: NodeMetadata, indexed: IndexReadView,
  current: CanonicalSnapshot, filters: X1Filters)
  -> Result<{flags: Set<X1Flag>, matches: bool}, DomainError>     // I-7
```

InclusiveLines: start/end целые, 1≤start≤end, обе включительны. Без диапазона читаются bytes всего файла. Для новых источников validate_basis требует доступного полного диапазона и сравнивает bytes по всем итоговым связям в одном основании; conflict при изменении, validation_failed при недоступной новой привязке. Неверный тип OID — invalid_request, отсутствующий существующий по форме объект — not_found; повреждение Git — internal_error, не маскируется.

Observe не подтверждает знание и не пишет новый source_snapshot. Flags из существующего X-1, совпадение source+revision проверяется на одной привязке; исторический hash вычисляется из Git, не берётся из текущего source hash. Без обнаруженной причины flags пуст; это не проверка истинности. Чтение одинакового path внутри вызова можно разделить временным владением, постоянного source cache нет. Веб-загрузчик в порт не входит.

Вход-пример: source_snapshot=S1, source=src/a.rs#L2-L5; current=S2. Равные адресованные bytes дают matches=true для фильтра S1 даже при другом commit, неравные — source_changed. Исчезнувший диапазон даёт source_not_found и сохранение старого знания; новая привязка к нему отклоняется. Выход только метаданные/состояние, не исходный текст.

До успешного сохранения validate_basis передаёт каждый принимаемый source_snapshot Git-порту для отдельного pin+harden в существующем refs namespace. Его равные адресованные bytes не означают равенства всего tree before/after; ссылочная достижимость basis обязательна независимо от них.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit. No mutation replay is performed by this port.
