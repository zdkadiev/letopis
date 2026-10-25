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

Локальный адрес отклоняется, если после нормализации `.`/`..` и слешей путь
начинается с `http:` или `https:` без учёта регистра: например `./https:notes`,
`dir/../HTTP:notes`, `./https://Host/a`, в том числе с диапазоном строк. Такой
адрес не превращается в URL: его сохранённый ID иначе менял бы классификацию
при повторном чтении. Принятый ID повторно разбирается с тем же адресом и типом.
Корневой файл с таким префиксом нужно переименовать или поместить в однозначный
каталог (`docs/https:notes`). Грамматика RepoPath/KnowledgeId и исходное написание
настоящих абсолютных HTTP/HTTPS URL не меняются. Неверная external-ссылка следует
обычной политике invalid-file: прежняя валидная проекция сохраняется, а в свежий
кэш некорректный source/edge не попадает; исправление ссылки не требует пересборки.

## Реализованная граница TK-21 — 2026-10-25

[Утверждённая спецификация](../../../superpowers/specs/2026-10-25-tk-21-provenance-design.md)
и [отчёт проверки](../checks/TK-21/report.md) уточняют исторический Layout ниже.
В `service/mod.rs` объявлены `SourceProvenance` и `BasisRetainer`; конкретный
`ProvenanceService<G,F>` находится в `service/provenance.rs`.

```rust
SourceProvenance::observe_sources(ProvenanceRequest {
    source_snapshot: Option<&TreeOid>, sources: &BTreeSet<SourceUri>,
}) -> DocumentResult<ProvenanceObservation>
SourceProvenance::compare_revision(SourceRevisionRequest {
    source_snapshot: Option<&TreeOid>, source: &SourceUri, revision: &TreeOid,
}) -> Result<SourceRevisionComparison, SnapshotError>
BasisRetainer::retain_snapshots(&BTreeSet<TreeOid>) -> DocumentResult<()>
```

`ProvenanceObservation` содержит только `BTreeSet<SearchFlag>` и
`BTreeSet<ProvenanceIssue>`. Caller передаёт snapshot и source targets одного
pinned view. Наблюдение не открывает Storage, не refresh/pin/write и не возвращает
bytes. Check и Search используют тот же read view для metadata, bindings и
ранжирования; live bytes читаются отдельно и не имеют межфайловой атомарности.

Refresher под существующим index.lock проверяет/закрепляет каждый доступный
эффективный basis до SQL write transaction, включая last-valid metadata и ранее
отсутствовавший tree при неизменном Markdown. Git I/O/corruption/pin failure
прерывают публикацию, отсутствующий/невалидный root остаётся диагностикой.
Корректный ref может остаться после SQL rollback. Markdown не изменяется.
Сохранённое поле snapshot удерживает прежнее основание при edit; удалённое поле
означает отсутствие основания, которое пересборка кэша не восстанавливает из истории.

Точные bytes всего файла или включительного диапазона сравниваются без
UTF-8/EOL нормализации. Отличие — `source_changed`, отсутствующий полный адрес
в current или basis — `source_not_found`. Неизвестный basis — `source_changed`
как причина перепроверки, с `missing_source_snapshot` либо
`source_snapshot_unavailable` в check; независимые причины не теряются.
Check добавляет `source_not_found` по owner path/element_id/target, сортирует и
дедуплицирует findings. Search объединяет эти flags с `stale`, сохраняя ровно
`element_id/path/lines/revision/flags`; знание остаётся находимым при ошибке источника.
HTTP/HTTPS остаются `Unfetched`, без ложного 404 и без локального/сетевого чтения.

`compare_revision` сравнивает одну и ту же пару source+revision с basis данного
знания: Equal/Different/SourceNotFound/BasisUnavailable/Unfetched. Явно запрошенный
root проверяется первым: missing — NotFound, wrong type/format — InvalidRequest,
corruption — InternalError. Разные trees с равными адресованными bytes равны.
JSON search filters остаются TK-22, source в CLI inspect — TK-24; эти consumers
не заявляются реализованными. Нет новой schema/history/cache/процесса/runtime.

Следующий Layout сохраняет исторические имена и более широкий целевой контракт;
это не дополнительные существующие Rust APIs и не доказательство готовности filters.

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

InclusiveLines: start/end целые, 1≤start≤end, обе включительны. Без диапазона читаются bytes всего файла. Validate_basis не отказывает индексации [D-93](../decisions.md#D-93): он сравнивает bytes по всем связям знания в одном основании; несовпадение даёт флаг source_changed, недоступный tree или диапазон — диагностику file:<путь знания> и флаг перепроверки. Для явно запрошенной revision неверный тип OID — invalid_request, отсутствующий существующий по форме объект — not_found; повреждение Git — internal_error, не маскируется.

Observe не подтверждает знание и не пишет новый source_snapshot. Flags из существующего X-1, совпадение source+revision проверяется на одной привязке; исторический hash вычисляется из Git, не берётся из текущего source hash. Без обнаруженной причины flags пуст; это не проверка истинности. Чтение одинакового path внутри вызова можно разделить временным владением, постоянного source cache нет. Веб-загрузчик в порт не входит.

Вход-пример: source_snapshot=S1, source=src/a.rs#L2-L5; current — рабочая копия. Равные адресованные bytes дают matches=true для фильтра S1 даже при другом commit, неравные — source_changed. Исчезнувший диапазон даёт source_not_found и сохранение знания; новая ссылка на него индексируется с этим флагом и диагностикой. Выход только метаданные/состояние, не исходный текст.

При индексации validate_basis передаёт каждый найденный в метаданных source_snapshot Git-порту для pin в существующем refs namespace ([D-93](../decisions.md#D-93)); отсутствующий tree — диагностика и признак перепроверки, не отказ. Ссылочная достижимость basis обязательна независимо от совпадения bytes.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit.
