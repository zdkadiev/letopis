# TK-21 Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Закреплять объявленные основания знаний в Git и показывать корректные признаки перепроверки в существующем поиске, с общим сравнением ревизий для TK-22.

**Architecture:** Общий application service сравнивает адресованные raw bytes через Git/FileSystem interfaces; владелец сценария передаёт согласованные метаданные и ссылки. Индексатор отдельно удерживает основания до cache commit, check/search вычисляют текущие состояния без записи flags в БД. Реальные collaborators собираются в lib.rs, CLI только форматирует результат.

**Tech Stack:** Существующие Rust 1.95.0, synchronous std I/O, Git CLI, rusqlite/SeaQuery, MarkdownParser; без новых dependencies/runtime/schema.

**Spec:** [Утверждённая спецификация TK-21](../specs/2026-10-25-tk-21-provenance-design.md), approved 2026-10-25 («go on», включая два уточнения).

**Execution:** approved — Subagent-driven. Пользователь утвердил план и затем заменил первоначальный выбор Native указанием «no go subagent». Все 16 шагов выполнены; три task reviews и независимый whole-branch final review approved. Product revision: `cd1d94ab7709901d34bdb29dbf7d226c51d982eb`.

## Global Constraints

- Ветка `tk-21`, worktree `/Users/zdkadiev/.codex/worktrees/tk-21/letopis`; base `9b798be522782e64375f4cbd293b76778e517117`. Не менять другие worktrees.
- Пользователь утвердил: проверка происхождения при индексации и flags существующего поиска входят в TK-21; поисковые filters остаются TK-22, source в CLI inspect — TK-24.
- `source_snapshot` — один необязательный полный tree OID для всех external-связей знания; сравниваются адресованные bytes, не OID целиком. Поддерживаются SHA-1/SHA-256 repositories.
- Letopis не пишет Markdown, не подставляет HEAD/inspect snapshot, не наследует скрытое основание только из БД. Удалённое поле означает отсутствие канонического основания.
- HTTP/HTTPS остаются `Unfetched`: не читаются как локальные пути и не скачиваются.
- Flags вычисляются при чтении, дедуплицируются и не сохраняются как жизненный цикл в SQLite. Неизвестное/неуказанное основание даёт существующий `source_changed` плюс точную причину check.
- Текст источников, excerpts и новые JSON-поля в выдачу не добавляются. Нет нового CLI/process, registry, history/schema migration, постоянного кэша источников, web loader или нового формата Git refs.
- Caller владеет refresh и pinned view. Метаданные и external-связи одного чтения берутся из одного view; live bytes наблюдаются отдельно и не получают межфайловой гарантии.
- Длинная проверка/закрепление Git выполняется под существующим index.lock до SQL write transaction; retain не меняет пользовательские HEAD/index/ветку. Уже закреплённый ref допустим после SQL rollback.
- Сохранять обработку invalid-file/last-valid, source membership/min created_at/orphans, revision и contentless FTS; не хранить search text всего корпуса в памяти ради подготовки публикации.
- Обязательны AGENTS.md: реальные trait/struct boundaries, caller выше helpers, explicit imports/Debug/docs, verbs/get_, ~40/максимум 60 строк методов, AAA и независимые ожидаемые значения. Не добавлять интерфейсы/файлы будущих задач.
- Проверки offline/locked, shared target `/Users/zdkadiev/Repos/letopis/target`, `RUST_TEST_THREADS=2`. Полный output сохранять в новом `.artifacts/tk21/` evidence; в чат — краткий итог, полный вывод только при ошибке.
- Общий Engram: `ENGRAM_DB=/Users/zdkadiev/Repos/letopis`, существующая задача `entity_db60b745fe8e42afbc29e59d0ef6d385`; сохранять все attrs, exact optimistic tokens и чужие изменения.
- G-1/TK-17, ресурсные gates, G-2 и готовность v1 этим выполнением не объявляются пройденными. Merge/push/GitHub mutation — по отдельной авторизации.

## Review Focus

1. Ранее отсутствовавший tree появляется при неизменном Markdown: следующий refresh закрепляет его, не публикуя новую SQL revision — Task 2, `retain_newly_available_basis_without_markdown_change`.
2. Доступный root содержит повреждённый child либо update-ref отклонён: это infrastructure failure с сохранением прежнего кэша и пользовательского Git state — Task 2, `reject_corrupt_closure_and_ref_failure_before_publication`.
3. Невалидная правка знания сохраняет старую проекцию/basis, а изменение только source замечается и без новой SQL revision — Task 2 last-valid и Task 3 live-source regressions.
4. Несколько диапазонов одного файла, binary/CRLF/EOF, одна отсутствующая и одна изменённая привязка: сравнение точное, flags без дублей, пары не смешиваются — Task 1 observation/revision cases.
5. Между чтениями публикуется другая revision: check/search используют метаданные и external-привязки только исходного pinned view, без второго storage read/скрытого pin — Task 3 pinned-view recording tests.

---

## File Map and Interfaces

| Файл | Ответственность | Task |
| --- | --- | --- |
| `src/service/mod.rs` | Объявить interfaces и необходимые types/reexports, без implementation bodies | 1 |
| `src/service/provenance_types.rs` (новый) | Документированные typed requests/outcomes, отделённые от реализации | 1 |
| `src/service/provenance.rs` (новый) | `ProvenanceService<G,F>`: raw observations, revision comparison, retention | 1 |
| `src/service/provenance/tests.rs` (новый) | Private boundary fakes/error mapping/no-I/O and retention tests | 1 |
| `src/lib.rs` | Реальные constructor calls: отдельный read API и collaborators потребителей | 1–3 |
| `src/service/document_refresher.rs` | Retention gate и предварительная проверка перед существующей публикацией | 2 |
| `src/service/document_refresher/basis.rs` (новый) | Узкая preflight-подготовка basis из cached graph/изменённых файлов, без накопления текстов | 2 |
| `src/service/projection.rs` | Доступ к effective metadata и проверка согласованности preflight; не новая history | 2 |
| `src/service/document_refresher/tests.rs` и `tests/process.rs` под его tests module | Обновить owned fakes/constructor и сохранить существующие crash/race checkpoints | 2 |
| `src/service/check.rs`, `src/service/check/tests.rs` | Общая provenance diagnostics вместо частного root-only алгоритма | 3 |
| `src/service/search.rs`, `src/service/search/tests.rs` | Объединить прежний stale с live source flags из того же view | 3 |
| `src/service/command.rs`, `src/service/command_traits.rs`, `src/cli/output.rs` | Два существующих X-1 flag values (Task 1) и точные комментарии consumers (Task 3) | 1, 3 |
| `tests/support/provenance.rs` (новый), `tests/support/mod.rs` | Общие private Git/Markdown fixtures, минимум два реальных потребителя | 1–3 |
| `tests/provenance_observation.rs`, `tests/provenance_revisions.rs` (новые) | Состояния/bytes и equality одной пары source+revision | 1 |
| `tests/provenance_indexing.rs` (новый) | Pin, unchanged refresh, metadata edits, rebuild, rollback и GC | 2 |
| `tests/provenance_commands.rs` (новый), существующие `tests/check_command.rs`, `tests/source_projection.rs` | Настоящие CLI contracts и скорректированные ожидаемые текущие findings | 3 |
| I-4, MC-4, TK-21 card, `checks/TK-21/report.md` и `verification.json` (новые) | Фактические границы и acceptance evidence без переписывания исторических receipts | 3 |

`document_refresher/tests/process.rs` означает существующий файл
`src/service/document_refresher/tests/process.rs`; не создавать новый runner.
Незатронутые Git/storage/Markdown реализации не перестраивать ради стиля.

### Общий API (определяется Task 1)

В `provenance_types.rs`, с `Debug` и документацией каждого public item:

```rust
pub struct ProvenanceRequest<'a> {
    pub source_snapshot: Option<&'a TreeOid>,
    pub sources: &'a BTreeSet<SourceUri>,
}
pub struct ProvenanceObservation {
    pub flags: BTreeSet<SearchFlag>,
    pub issues: BTreeSet<ProvenanceIssue>,
}
pub enum ProvenanceIssue {
    MissingSnapshot,
    SnapshotUnavailable { snapshot: TreeOid },
    SourceNotFound { source: SourceUri },
}
pub struct SourceRevisionRequest<'a> {
    pub source_snapshot: Option<&'a TreeOid>,
    pub source: &'a SourceUri,
    pub revision: &'a TreeOid,
}
pub enum SourceRevisionComparison {
    Equal, Different, SourceNotFound, BasisUnavailable, Unfetched,
}
```

`ProvenanceIssue` имеет `Eq/Ord` для дедупликации. `SearchFlag` в Task 1 уже
расширяется `SourceChanged`/`SourceNotFound`, вместе с обязательными arms formatter;
подключение к выдаче — Task 3. Это позволяет независимо проверять Task 1 без
временного второго enum или некомпилирующегося exhaustive match.

В `service/mod.rs` только declarations, structs/impl — в указанных файлах:

```rust
pub trait SourceProvenance {
    fn observe_sources(&self, request: ProvenanceRequest<'_>)
        -> DocumentResult<ProvenanceObservation>;
    fn compare_revision(&self, request: SourceRevisionRequest<'_>)
        -> Result<SourceRevisionComparison, SnapshotError>;
}
pub trait BasisRetainer {
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;
}
```

Read contract: request собирает caller из метаданных и source targets одного pinned
view (или одной проверенной проекции при индексации); сервис не открывает Storage,
не refresh/pin/write и возвращает только метаданные. Один SourceUri используется
в обоих trees для compare. Unknown requested revision проверяется **до** возврата
`BasisUnavailable`/`Unfetched`; wrong type/format — InvalidRequest, missing — NotFound.

Write contract: caller `BasisRetainer` держит index.lock. Каждый уникальный root
проверяется через SnapshotVerifier; Available требует успешного GitSnapshots::pin,
Missing/Invalid остаются диагностируемым состоянием, InternalError не подавляется.
После Available отказ pin — infrastructure failure, не успешная публикация.

Concrete: `pub(crate) struct ProvenanceService<G,F>` и `new(git: G, filesystem: F)`;
`G: GitSnapshots + SnapshotVerifier`, `F: FileSystem`. Публичная сборка в lib.rs:
`wire_source_provenance(root: &Path) -> DocumentResult<impl SourceProvenance + use<>>`.
Она возвращает только read capability; внутренние consumers получают concrete
service через настоящие constructor calls, без нового глобального registry/dyn.

---

### Task 1: Общие наблюдения происхождения и сравнение ревизий

**Files:** новые provenance types/service/tests/support и `src/service/mod.rs`,
`src/service/command.rs`, `src/cli/output.rs`, `src/lib.rs` из карты.
**Consumes:** существующие `GitSnapshots`, `SnapshotVerifier`, `FileSystem`,
`SourceUri::get_local`, `LocalSource::{get_path,select_bytes}`, `Sha256::digest`.
**Produces:** точные interfaces/types выше; `wire_source_provenance`; typed
observation/compare/retain, без привязки к CLI workflow или storage lifetime.

- [x] **Step 1: Написать failing observation/revision tests.** Общий helper
  `create_basis(root: &Path, files: &[(&str, &[u8])]) -> TreeOid` создаёт дерево через
  существующий GitSnapshots; `build_note(snapshot: Option<&TreeOid>, sources: &[&str], body: &str) -> String`
  создаёт явную Markdown fixture. Helpers не вычисляют ожидаемые flags/равенство.
  Центральные assertions (fixtures заданы тестом, expected — literals):

```rust
// observe_exact_binary_range_and_ignore_unaddressed_edits
// Basis: b"prefix\r\n\0abc\r\nlast"; current: b"edited\n\0abc\r\nother".
// Source address in BOTH states: "src.bin#L2-L2".
assert_eq!(observation.flags, BTreeSet::new());
assert_eq!(observation.issues, BTreeSet::new());
// After changing addressed bytes to b"\0xyz\r\n":
assert_eq!(changed.flags, BTreeSet::from([SearchFlag::SourceChanged]));
// compare_different_trees_by_one_addressed_pair
assert_eq!(same_range, SourceRevisionComparison::Equal);
assert_eq!(different_range, SourceRevisionComparison::Different);
assert_eq!(absent_range, SourceRevisionComparison::SourceNotFound);
```

  Добавить отдельные tests: `combine_changed_and_missing_sources_without_duplicates`,
  `distinguish_missing_basis_from_missing_source`, `observe_empty_and_unterminated_ranges`,
  `leave_remote_unknown_without_file_reads`, `reject_missing_or_invalid_requested_revision_first`.
  Проверить `MissingSnapshot`/`SnapshotUnavailable` + SourceChanged, отсутствие локального
  current или historical path/range + SourceNotFound, обе причины одновременно.
  Unknown remote с доступным basis не даёт ложных flags; no external/no snapshot — пусто.
  Проверить C1/C2 на SHA-1/SHA-256 fixtures и две несовпадающие привязки, которые
  нельзя случайно скрестить. InvalidRequest не превращать в NotFound; corruption и
  unsafe/I/O сохраняют private cause и безопасное сообщение.

- [x] **Step 2: Запустить RED.**
  `cargo test --locked --offline --test provenance_observation --test provenance_revisions`.
  Сначала API может отсутствовать; после добавления минимальных declarations получить
  хотя бы один runtime RED на сравнении/классификации. Сохранить фактическую причину,
  не выдавать compile failure за доказательство поведения.

- [x] **Step 3: Реализовать API из блока выше.** Для локального адреса читать complete
  Git blob, выбирать bytes через существующий LocalSource, сравнивать точные SHA-256
  выбранных bytes с live capture (не stored source hash). Приватные helpers
  `read_snapshot_hash(&self, tree: &TreeOid, source: &SourceUri) -> Result<Option<Sha256>, SnapshotError>`
  и `read_current_hash(&self, source: &SourceUri) -> DocumentResult<Option<Sha256>>`
  применяются только к local; Unfetched обрабатывается раньше. Verify root отличает
  missing/invalid от corrupt; одинаковые roots не проверять повторно внутри одного
  observation. Проверить retention через private fake: once-per-unique-OID, no pin
  missing/invalid, pin failure propagated. Observe/compare вообще не вызывают pin.

- [x] **Step 4: GREEN, control, архитектурная проверка.** Повторить две test binaries
  и `cargo test --locked --offline --lib service::provenance::tests`, затем `make check`.
  По одному control на подмену адресованного equality OID-равенством и подавление
  missing requested root; тесты обязаны упасть. Восстановить код и повторить focused
  tests. Проверить отсутствие SQL/HTTP/new runtime, actual lib.rs wiring и видимость.

- [x] **Step 5: Commit.** Добавить только перечисленные Task 1 paths;
  `git commit -m "feat(tk-21): observe source provenance and compare addressed revisions"`.
  Записать SHA/команды/controls в Engram и локальный ledger.

### Task 2: Проверка и удержание оснований до публикации

**Files:** `document_refresher.rs`, новый `document_refresher/basis.rs`, `projection.rs`,
их существующие private tests, `service/mod.rs`, `lib.rs`, новый `tests/provenance_indexing.rs`.
**Consumes:** SourceProvenance/BasisRetainer Task 1; Refresher, FileProjection,
PreparedProjection, EffectiveGraph и существующие index.lock/checkpoints.
**Produces:** `DocumentRefresher<G,F,M,S,P>` с `P: SourceProvenance + BasisRetainer`,
constructor `new(dependencies: RefreshDependencies<G,F,M,S,P>) -> Self`;
`RefreshDependencies` содержит поля `git/ filesystem/ markdown/ storage/ provenance`
соответствующих generic types, объявляется рядом в document_refresher.rs и
реэкспортируется crate-private через service/mod.rs для actual wiring;
`Refresher::refresh`/RefreshOutcome wire API без новых persisted/public JSON fields.

- [x] **Step 1: Написать failing indexing tests.** Имена/ключевые assertions:

```rust
// retain_basis_before_publishing_and_keep_history_after_gc
assert_eq!(read_retained_bytes(&repo, &old_tree), b"old source\r\n");
assert_eq!(repo.read_state().get_node("note.md").metadata, expected_metadata);
// retain_newly_available_basis_without_markdown_change
assert!(!second.published);
assert_eq!(second.revision, first.revision);
assert!(has_exact_service_ref(&repo, &previously_missing_tree));
// reject_corrupt_closure_and_ref_failure_before_publication
assert!(refresh_result.is_err());
assert_eq!(repo.read_state(), before);
assert_eq!(capture_head_and_index(&repo), user_state_before);
```

  `read_retained_bytes` использует existing Git read_blob, expected bytes — literal;
  остальные fixture helpers проверяют Git state независимо от кода удержания:
  `has_exact_service_ref(repo: &TempRepo, tree: &TreeOid) -> bool` использует Git
  show-ref/exact OID, `capture_head_and_index(repo: &TempRepo) -> FixtureState` —
  существующий capture_fixture_state. `read_retained_bytes(repo: &TempRepo, tree: &TreeOid) -> Vec<u8>`
  читает фиксированный fixture path `source.txt`. Для real Git tests переиспользовать
  TempRepo/FORMATS; source paths и исходные bytes создаются явно каждым тестом.
  Создавать refs/GC/ошибочные objects только в disposable repositories.
  Добавить `preserve_declared_basis_on_edit_and_reproduce_it_after_cache_rebuild`,
  `remove_basis_only_when_metadata_field_is_removed`, `retain_last_valid_basis_after_invalid_edit`,
  `retain_shared_and_unreferenced_declared_basis_once` и boundary-event tests:
  index lock → preflight/Git pin → begin_cache_write → commit; read failure/pin
  failure не начинают успешную SQL-публикацию. Пропавший root публикуется с исходным
  OID; его последующее появление проверяется без новой Markdown/SQL revision.

- [x] **Step 2: RED.** `cargo test --locked --offline --test provenance_indexing`.
  Получить runtime failure отсутствующего ref/ошибочно успешной публикации. Сохранить
  counts и причину; не менять существующие race/crash assertions ради GREEN.

- [x] **Step 3: Добавить preflight и retention gate.** Сохранить существующую схему
  detect → index.lock → redetect. Под lock собрать старые эффективные основания и
  новые основания только валидных изменённых файлов, проверяя captured stat/hash
  против Observation. Предварительно прочитать/разобрать по одному изменённому файлу,
  сохранить только metadata/source addresses, сбросить bytes/search text перед следующим.
  Для новых valid элементов вызвать observe_sources: состояния missing/changed не
  запрещают публикацию, unsafe/Git/I/O ошибки запрещают. Flags не сохранять: read
  consumers наблюдают live состояние повторно; это не скрытый lifecycle cache.

  В `basis.rs` concrete private `BasisPreflight` с полем `snapshots: BTreeSet<TreeOid>` и
  `prepare_basis<F: FileSystem, M: MarkdownCodec>(filesystem: &F, markdown: &M,
  request: BasisPreparation<'_>) -> DocumentResult<BasisPreflight>`;
  `BasisPreparation<'a> { graph: &'a EffectiveGraph, updated: &'a BTreeMap<RepoPath, Observation> }`.
  `BasisPreflight` дополнительно владеет `bases: Vec<PreparedBasis>`, где
  `PreparedBasis { source_snapshot: Option<TreeOid>, sources: BTreeSet<SourceUri> }`;
  это только metadata, без исходных/search bytes. Метод
  `BasisPreflight::validate_basis<P: SourceProvenance + BasisRetainer>(&self, provenance: &P) -> DocumentResult<()>`
  выполняет observation новых valid bases и retention snapshots, без Storage.
  `EffectiveGraph::list_source_snapshots(&self) -> BTreeSet<TreeOid>` даёт основания
  прежней эффективной проекции; removed old roots можно удержать как исторические.
  `FileState` добавляет `source_snapshots: BTreeSet<TreeOid>`, прочитанное вместе с
  files/revision из одного read в read_file_state, для корректного unchanged gate.
  Public interfaces Task 1 не менять.
  Pin всего собранного множества выполнить до begin_cache_write. Последующее обычное
  prepare_file обязано подтвердить тот же observed hash/stat: новая смена snapshot
  между preflight и публикацией даёт fail_changed, не публикацию неприкреплённого basis.
  Можно удержать старый уже неиспользуемый tree — refs сохраняются бессрочно.

  В unchanged path при непустом множестве basis получить index.lock, перечитать
  актуальное состояние, проверить/закрепить roots и вернуть published=false с прежней
  revision; пустой basis сохраняет существующий быстрый путь. Никакой фиктивной SQL
  write transaction ради проверки refs. Обновить lib.rs concrete assembly и только
  затронутые fakes/constructors; Refresher не получает прямой concrete GitCli.
  Цена bounded preflight — дополнительный read/parse изменённого файла перед SQL;
  старые test counters обновляются только на эту объяснённую работу. Unchanged
  файлы не перепарсивать, race/rollback/lock assertions сохраняются.

- [x] **Step 4: GREEN и controls.** Выполнить `cargo test --locked --offline --test provenance_indexing --test document_refresh --test source_lifecycle`,
  private `cargo test --locked --offline --lib service::document_refresher::tests` и
  `make check`. Controls: убрать retain call; вернуть early return для неизменного
  Markdown с missing→available tree. Соответствующие tests падают; восстановить код,
  повторить affected tests. Проверить, что source bytes/search text не накапливаются
  на весь корпус и existing atomicity/revision/checkpoints сохранены.

- [x] **Step 5: Commit.** Только Task 2 files и явно нужные constructor fixtures;
  `git commit -m "feat(tk-21): retain declared source bases before cache publication"`.
  Зафиксировать runtime RED/GREEN, controls и actual scope в Engram/ledger.

### Task 3: Check/search, end-to-end и acceptance report

**Files:** check/search services/tests, command traits, lib.rs, новые provenance CLI
regressions и docs из карты; formatter values уже существуют после Task 1.
**Consumes:** Task 1 request/outcomes и Task 2 refresher; один StorageRead у каждого
consumer; существующие X-1 output envelope, owner selection и GraphDiagnostics.
**Produces:** check + search с live provenance; неизменные JSON fields, две дополнительные
допустимые строки flags; полный TK-21 report/receipt с границами последующих задач.

- [x] **Step 1: Написать failing CLI и pinned-view tests.**

```rust
// search_observes_source_only_changes_without_new_cache_revision
assert_eq!(result[0]["flags"], json!(["source_changed"]));
assert_eq!(result[0]["revision"], initial_revision);
// missing_source_remains_searchable_and_check_reports_owner
assert_eq!(result[0]["element_id"], "note.md");
assert_eq!(result[0]["flags"], json!(["source_not_found"]));
assert_eq!(problem["details"]["path"], "note.md");
assert_eq!(problem["details"]["reason"], "source_not_found");
// missing_basis_is_diagnosed_without_dropping_knowledge
assert_eq!(result[0]["flags"], json!(["source_changed"]));
assert_eq!(problem["details"]["reason"], "missing_source_snapshot");
```

  Actual CLI fixtures используют отключённую embedding config, не платную модель.
  Проверить exact Link keys `element_id/path/lines/revision/flags`, без текста/excerpts;
  явный metadata update снимает source_changed только при равных bytes. Проверить
  snapshot_unavailable, missing historical range, сочетание stale + source flags,
  stable sort/dedup и owner paths; remote Unfetched не маскируется под 404.
  Private recording tests публикуют другую revision после begin_read и доказывают,
  что последующие metadata/edges прочитаны прежним view; observe не open/write/pin.
  Скорректировать старые check fixtures, где новые provenance findings теперь
  ожидаемы: объяснить каждое изменение, не ослаблять assertions до contains-only.

- [x] **Step 2: RED.** `cargo test --locked --offline --test provenance_commands` и
  targeted private check/search cases. Получить фактическое отсутствие flags/findings,
  а не ограничиться compile failure нового constructor.

- [x] **Step 3: Подключить consumers и concrete wiring.**
  `CheckService<D,S,G,P>::new(refresher: D, storage: S, diagnostics: G, provenance: P)`
  заменяет прежний generic V:SnapshotVerifier; private root-only SnapshotChecks
  удаляется, не остаётся вторым алгоритмом. Caller группирует Source targets
  list_edges из своего view по knowledge ID, observe_sources для выбранных знаний,
  переводит issues в CommandProblem с существующими path/element_id/target и reason:
  missing_source_snapshot/source_snapshot_unavailable/source_not_found. Сохраняются
  ValidationFailed, безопасные сообщения, полная проверка графа до owner selection.

  `SearchDependencies<D,S,E,F,M,P>`/`SearchService<D,S,E,F,M,P>` получают field
  `provenance: P`, `P: SourceProvenance`. Перед build_link взять source bindings
  ранжированных знаний из того же read view; объединить observed flags со stale.
  Не открывать вторую SQL read/write, не pin refs в observe; не менять rank/limit,
  query provider semantics или существующую несовместимость filters до TK-22.
  В lib.rs собрать actual ProvenanceService из отдельных Git/FileSystem instances;
  обновить комментарии Checker/Searcher и public reexports. Consumer helpers не
  импортируют concrete adapters. Отсутствие basis не делает knowledge invalid-file.

- [x] **Step 4: Focused GREEN, controls и итоговое ревью.**
  `cargo test --locked --offline --test provenance_observation --test provenance_revisions --test provenance_indexing --test provenance_commands --test check_command --test check_lifecycle --test knowledge_search --test search_consistency --test search_contention --test source_projection`;
  затем relevant private check/search tests. Controls: удалить merge source flags
  в search и подавить check SourceNotFound issue; tests обязаны упасть. Восстановить
  код, повторить affected suites. Проверить реальные import directions, visibility,
  structs/impl ownership, lib.rs constructors, function/file sizes и safe error causes.
  Итоговое independent whole-branch review по выбранному execution workflow; реальные
  findings исправить с focused regressions перед окончательным verification run.

- [x] **Step 5: Fresh verification и документация.** Выполнить один полный `make verify`
  (`make verify V=1` с выводом в новый evidence log) на фактическом финальном tree, сохранить source-input hashes и точные
  counts с исключением вложенных subprocess summaries. Проверить schema/native/build
  diff: при их изменении нужен `python3 -B tests/storage/run.py --platform host --evidence <new-directory>`;
  без этих изменений runner не требуется и не объявляется пройденным. Отдельно
  отметить неисполненные ignored/исторические отдельные harnesses и platform limits.

  Обновить I-4/MC-4 реализованными сигнатурами и approved semantics, карточку TK-21
  утверждённой границей и actual results. Создать `docs/zkit/letopis/checks/TK-21/report.md`
  и `verification.json`: каждому критерию spec/card — test/command/result/evidence,
  final product revision/input hashes, controls/restoration, architectural audit и
  remaining TK-22/TK-24 scopes. Ссылки/JSON/diff проверить после документационных
  правок; неизменённые product suites повторно без причины не гонять.

- [x] **Step 6: Commit и handoff.** Добавить только Task 3 paths/report/docs;
  `git commit -m "feat(tk-21): expose live provenance diagnostics and search flags"`.
  При doc-only acceptance metadata использовать отдельный точный commit. Сохранить
  результаты/commits в существующую задачу Engram с сохранением attrs и provenance;
  сообщить полный статус, реальные limits и оставшееся. Не merge/push/close issue
  без соответствующего запроса пользователя.

## Self-review / acceptance map

| Spec | Реализация и проверка |
| --- | --- |
| §2/§8 boundaries, synchronous dependencies, lib.rs | Tasks 1–3 interfaces/file map; physical audit Task 3 |
| §3 canonical edit/removal/rebuild | Task 2 edit/rebuild tests, no Markdown writes |
| §4 pin/closure/unchanged/missing/rollback/history | Task 1 retainer fakes; Task 2 preflight/real Git/GC/old-view tests |
| §5 exact bytes/ranges/current changes/remote | Task 1 observation/revision tests; Task 3 source-only CLI |
| §6 flags/issues/owner selection/stale/dedup | Task 1 typed outcomes; Task 3 CLI + pinned view |
| §7 pair equality/request errors/C1-C2 | Task 1 revision tests; filters deliberately TK-22 |
| §9 negative paths/controls/evidence/limits | controls each Task, fresh verify/report Task 3 |

Self-review исполнителя: все три Tasks имеют самостоятельный runtime check и commit;
сигнатуры между Tasks согласованы; formatter расширяется вместе с enum в Task 1;
пять Review Focus cases закреплены конкретными tests. План — три зависимых изменения
одного сценария происхождения, не три независимых продукта. Код, зависимости и
schema в ходе планирования не менялись.

Самопроверка плана от 2026-10-25: устранены три конкретных planning gaps до передачи:
Task 1 одновременно добавляет enum variants и formatter arms; constructor с пятью
collaborators заменён request struct; private preflight types/signature и стоимость
повторного чтения определены явно. Отдельный subagent plan review не выполнялся
и skill его не требует: здесь проведён self-review, не code review.

## Independent acceptance and execution closure — 2026-10-25

All three task gates and the independent whole-branch review are approved, with no open Critical/Important/Minor findings. The final review accepts all five Review Focus cases and reports **Ready to merge: Yes** for the approved TK-21 scope. This is acceptance evidence, not a merge/push action or a claim about broader gates.
Execution was approved Subagent-driven after the user superseded the initial Native choice; all 16 planned steps are complete. The verified product revision remains `cd1d94ab7709901d34bdb29dbf7d226c51d982eb`; reviewed head `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` adds acceptance docs only. This final reconciliation also changes documentation only.
Local ignored review/evidence receipts are retained against their captured revisions; SHA256 values below identify their exact contents. Historical input, binary and log hash tables remain intact.

| Review | Scope and verdict | Model / effort | Reviewed base → head | SHA256 |
| --- | --- | --- | --- | --- |
| [task-1](../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-1-review.md) | Task1 shared provenance API, exact byte observation, revision comparison, retention and read-only wiring. Spec compliant; task quality Approved; no open findings. | `gpt-6.1-sol` / `high` | `f3167b91e24efec51287b0f85d9b590033425c34` → `1d8b2e057401b3456ccc0e7185a04f75e28f1a91` | `c1f0f2480ca2ab3b4e56ebf13ff5102540f85bdf8a69f8966125ae624f89ae95` |
| [task-2](../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-2-review.md) | Task2 pre-publication retention gate, locking, unchanged refresh and last-valid basis; mandatory late cleanup carried to Task3. Spec compliant; task quality Approved; no open findings. | `gpt-6-astra` / `high` | `1d8b2e057401b3456ccc0e7185a04f75e28f1a91` → `16f837aa119dbb7c68c1af43093130bf4834b614` | `46ba3a163ae949ad17228c84992b90bacb55d59587f26dd9e93995a35cd0d300` |
| [task-3](../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-3-review.md) | Task3 check/search, X-1, same-view bindings, mandatory real-storage post-query metadata-error cleanup and acceptance evidence. Spec compliant; task quality Approved; no Critical/Important/Minor. | `gpt-6.1-sol` / `high` | `16f837aa119dbb7c68c1af43093130bf4834b614` → `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` | `5a38ac039b4c69f90c384baa6ea1af137270175b3b6bb9e5c429e9a95b97d5d7` |
| [final](../../../.superpowers/sdd/2026-10-25-tk-21-provenance/final-review.md) | Whole branch: all37 changed files including implementation, tests, spec/plan/contracts/card/report/receipt; all5 Review Focus cases satisfied. Ready to merge: Yes for approved TK21 scope; no Critical/Important/Minor. | `gpt-6-astra` / `high` | `9b798be522782e64375f4cbd293b76778e517117` → `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` | `05bd79b116991c1142e86e0aed3419aaed0026d83372e596d0f102d6a0db555a` |

Provider-failure evidence limit: the existing barrier test drops its temporary SearchService before checkpointing and alone does not establish cleanup while that service remains alive. The reviews inspected local reader ownership and RAII; the mandatory post-query metadata-error test separately proves live-service checkpoint/publication after a successful controlled query and real metadata failure. No stronger provider-live runtime claim is made.

JSON search filters remain TK-22; source CLI inspect remains TK-24. Linux/native-runner reruns, historical standalone harnesses, real 60-second deadline, corpus/paid/real-model pilot, resource gates, G-1/G-2 and v1 readiness remain unclaimed. No unchanged product suites were rerun for this documentation-only closure. No Engram/GitHub/merge/push action is performed here.
