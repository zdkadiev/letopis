# I-2 — подготовка и публикация проекций
> x-boundary: I-2 · x-direction: internal · x-status: draft
> x-serves: FR-5, FR-13, FR-27, FR-28, FR-29 · x-limits: []

## Layout

Внутрипроцессный Rust-порт, не wire API. Все методы возвращают Result<T, DomainError>; DomainError имеет code из X-1, безопасные message и details. ElementId/KnowledgeId/hash — типы X-1; revision — счётчик публикаций (u64); Paths — проверенные root-relative пути. Текст существует только во временных буферах.

```text
ProjectionInput { base_revision: Option<u64>, changed_paths: Set<Path>, full: bool }
PreparedProjection { base_revision: Option<u64>,
  nodes: Vec<NodeMetadata>, edges: Vec<EdgeMetadata>, removed_ids: Set<ElementId>,
  files: Vec<FileRow>, search: Vec<{id: KnowledgeId, uri: NormalizedUri, search_hash: Sha256}> }
prepare(input: ProjectionInput) -> PreparedProjection
publish(unit: &mut WriteUnit, prepared: PreparedProjection) -> ()
```

NodeMetadata — id/kind/uri/hash/created_at/source_snapshot/line_start/line_end/search_hash из storage-schema.sql, row_id сохраняется по UPSERT; id знания — путь или путь#якорь ([D-93](../decisions.md#D-93)); kind выводится: metavertex — знание с исходящими includes ([D-95](../decisions.md#D-95)). EdgeMetadata — source_id/target_id/type, ключ — тройка; хранятся все объявленные рёбра, без FK на цель и без признака активности. Висячие рёбра и includes-циклы отсекаются при чтении (MC-6) и показываются как проблемы I-11; publish рёбра других файлов не пересчитывает. FileRow — path/size/mtime/inode/hash и текст ошибки для невалидного файла. Для отсутствующего source hash=null, source_snapshot=null, search_hash=null. Search хранит только адреса/хеши; prepare не удерживает тексты полного корпуса.

Prepare читает изменённые файлы рабочей копии и не меняет БД; Git-снимок для публикации не создаётся (D-95). Publish присоединяется к переданной SQLite unit; не делает BEGIN/COMMIT, callback к редактору или модели. Caller держит flock index.lock и проверяет base ([D-94](../decisions.md#D-94)); unit увеличивает счётчик revision. changed_paths и removed_ids покрывают всю разницу опубликованного состояния → текущие файлы; при base_revision=None или full=true готовится полная проекция (reindex). Подготовка идёт по файлу ([D-93](../decisions.md#D-93)): невалидный файл сохраняет прежнюю проекцию, его FileRow получает текст ошибки. Задания embeddings не ставятся: работа выводится из search_hash без вектора (MC-3).

Вход-пример: base_revision=6, changed_paths={knowledge/a.md}, full=false. Выход: строки элементов a.md и их search text; после publish+caller COMMIT revision=7; если search_hash новый, drain найдёт его как текст без вектора, иначе используется прежний vector. При ошибке caller откатывает unit; файлы letopis не меняет.

## Timing and limits

Synchronous in-process call; no independent deadline or quota. The caller owns the existing read view or write unit. Computing embeddings never runs inside the write unit. Resource limits remain those of mechanics and resource-math.

## Errors

Storage/Git failures propagate as Result error using the existing X-1 code/message/details; no successful result is fabricated after I/O failure. Caller releases its read view or rolls back the write unit.
