# TK-14 End-to-End CLI and Agent Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Проверить реальный путь «решение со ссылкой на код → Markdown → CLI → graph/FTS → inspect snapshot → current vector», передать TK-15 минимальный воспроизводимый loader и начальный навык агента.

**Architecture:** Переиспользовать существующие application services и wiring из lib.rs. Production delta по умолчанию отсутствует: новые интеграционные тесты и фиксированные входы обнаруживают реальные пробелы, а исправления допускаются только под наблюдаемый failing case. Загрузка выполняется записью fixture в изолированный Git-репозиторий и настоящим CLI refresh, без нового API импорта и SQL seed.

**Tech Stack:** Существующий Rust 1.95.0 / edition 2024, SQLite/FTS5/vec1, Git argv, синхронный ureq, стандартные Rust test helpers; Python 3.11+ / jsonschema==4.26.0 только для существующего acceptance runner. Новые Cargo dependencies не требуются.

**Spec:** [Утверждённая спецификация](../specs/2026-10-24-tk-14-thin-slice-design.md), approved пользователем «Go» 2026-10-24.

**State:** Утверждён пользователем «Хорошо.» 2026-10-24; выполняется Native. Worktree `/Users/zdkadiev/.codex/worktrees/tk-14/letopis`, branch `codex/TK-14`, production base `ae7d2eb`.

## Global Constraints

- «Главный пример навыка — сохранить решение по коду со ссылкой на исходник»; поиск/переиспользование остаётся обязательным отдельным сценарием.
- «Tokio/daemon/IPC/новые зависимости не предлагаются»; синхронный I/O, один CLI binary и существующие kernel locks.
- «Seed vectors для приёмки запрещены». SQL и library refresher не заменяют product CLI loading; public Storage read API — только наблюдение.
- «Полученный tree содержит захваченные файлы знаний; он не считается снимком исходников». Basis tree берётся из проверенного committed source.
- «Полные source flags/inspect не используются для приёмки текущего slice». Check недоступен до TK-19, mixed/source inspect — до TK-24.
- «При отсутствии продуктового дефекта production Rust можно не менять». Никакого нового integration service или import command.
- «Короткие компилируемые ручные negative controls с восстановлением исходников, без полного cargo-mutants»; ошибка компиляции не доказывает behavioral RED.
- Соблюдать [AGENTS.md](../../../AGENTS.md), [физические границы](../../zkit/letopis/architecture.md#program-structure), caller-first порядок, 40/60 строк, verb names и независимые oracles.
- Все evidence directories новые, старые receipts не переписываются. Текущий baseline `make check V=1` PASS — только подготовка, не feature acceptance.

## Review Focus

1. Одинаковый исходник как `src/cache.rs` и `./src/cache.rs#L02-L02`: адреса нормализуются, whole/range различны, counts не дублируются (Task 1).
2. Знание правят после inspect: Git bytes по возвращённому tree остаются прежними, live bytes могут отличаться (Task 1).
3. Невалидный файл рядом с валидной правкой: старый корректный индекс сохраняется только для невалидного файла, inspect не выдаёт его за текущий (Task 2).
4. Успешный foreground при зависшем/отказавшем провайдере: ответ не ждёт document inference, попытки ограничены, corpus/FTS сохраняются (Task 2).
5. Устаревший результат inference и неподдерживаемые команды: vector не присваивается новому hash; навык не обещает check/source inspect и не подменяет basis tree knowledge snapshot (Tasks 2–3).

## Physical Files and Boundaries

| Файл | Ответственность / владелец |
|---|---|
| `tests/fixtures/markdown/thin_slice/cache.rs`, `cache.md.in`, `flow.md.in` (новые) | Только фиксированные source/Markdown inputs; Task 1 |
| `tests/support/thin_slice.rs` (новый) | Частный helper загрузки и чтения metadata; Tasks 1–2 |
| `tests/thin_slice.rs` local path module | Объявить helper только для этого test target; Task 1 |
| `tests/thin_slice.rs` (новый) | CLI loading/inspect/source/vector и failure/race scenarios; Tasks 1–2, helpers после тестов |
| `tests/cli/run.py`, `test_runner.py`, `Dockerfile`, `.dockerignore` (изменения) | Hash inventory, Linux copy, schema-validated actual product receipts; Tasks 1–2 |
| `tests/cli/README.md` (изменение) | Воспроизведение TK-14 и передача loader в TK-15; Task 1, итог Task 3 |
| `skills/letopis/SKILL.md` (новый) | Переносимый reference skill для агента; Task 3 |
| `docs/zkit/letopis/checks/TK-14/report.md` (новый), task card, spec и этот plan | Фактическая приёмка, решения, ограничения; Task 3 |

Не создавать отдельный importer, вторую schema или product source service. Фиксированные `.in` templates не являются корпусом: helper создаёт из них `.md` только в disposable repo. Существующие большие support-файлы не перестраивать. Если `tests/thin_slice.rs` приблизится к 500 строкам, сначала удалить повторяющийся setup в helper; не дробить каждый тест на отдельный файл.

## Task 1: Reproducible CLI Loading, Sources and Snapshot Reading

**Files:** первые семь строк таблицы, кроме skill; изменения runner здесь включают inventory/copy, сбор наблюдений — Task 2.

**Interfaces — consumes (существующие):**
- `ReadingRepo::new() -> ReadingRepo`, `write(&self, path: &str, bytes: &[u8])`, `get_root(&self) -> &Path`, `invoke(&self, args: &[&str], input: &[u8]) -> Output`, `open_storage(&self) -> impl Storage` в `tests/support/reading.rs`.
- `Storage::begin_read`, `StorageRead::{list_nodes,list_edges,get_revision,read_vector}`; `NodeMetadata::{Knowledge,Source}`. Использовать точные типы из `service/model.rs`.
- Git setup через `Command` + `isolate_command(&mut Command)`; любой `git add/commit` только в disposable fixture, не в рабочем репозитории пользователя.

**Interfaces — produces (test-only `tests/support/thin_slice.rs`):**
- `SliceFixture::new() -> Self`: создаёт `ReadingRepo`, записывает/коммитит source, материализует templates, ещё не вызывает product refresh.
- `get_repo(&self) -> &ReadingRepo`; `get_source_tree(&self) -> &TreeOid`.
- `load(&self) -> Value`: запускает product search с `{"query":"CacheScope","limit":10}`, проверяет транспорт/exit 0, возвращает envelope. Не вызывает library refresh или write API.
- `read_projection(&self) -> ProjectionSnapshot`; snapshot содержит `revision: u64`, `knowledge: BTreeMap<String, KnowledgeMetadata>`, `sources: BTreeMap<String, SourceMetadata>`, `edges: BTreeSet<(String,String,String)>`; handles отпускаются до следующего CLI/HTTP.
- `emit_slice(case: &str, command: &str, exit: i32, response: &Value)` печатает одну строку `THIN_SLICE_RESPONSE <json>` только после assertions; Task 2 runner читает этот формат.

- [x] **Step 1: Зафиксировать входы и независимые ожидания.**

`cache.rs` содержит ровно LF bytes:
```rust
pub fn use_cache() -> bool {
    true
}
```

Оба templates используют metadata `source_snapshot:"__SOURCE_TREE__"`; placeholder заменяется реальным committed source tree. `knowledge/cache.md` — whole-file элемент, created_at=10, body:
```markdown
CacheScope rebuilds derived state.
[source](src/cache.rs "letopis:type=external")
```
`knowledge/flow.md` — два раздела: `## Reuse {#reuse}` / created_at=20 с текстом `CacheReuse uses CacheScope.`, depends_on к `knowledge/cache.md`, external к `./src/cache.rs#L02-L02`; `## Scope {#scope}` / created_at=30 с текстом `CacheScopeMap groups decisions.`, includes к `knowledge/cache.md`, external к `src/cache.rs`. Якоря и metadata находятся на соседних строках по X-4, LF в конце каждого файла.

Независимые ожидания: 3 knowledge + 2 source = 5 nodes; 5 уникальных рёбер. Source IDs `src/cache.rs`, `src/cache.rs#L2-L2`; created_at 10 и 20; lines None и [2,2]. Whole source SHA-256 `551069b2bf2940ac6ce6ed7e27ee28b48097e5f13a70f132438ca4610b463e52`; range SHA-256 `8e0329bc53da6e5e8c823fd66d7840f3c8cbc6848040b04a58d1b10d651d5c17`.

Cache visible search text — literal `CacheScope rebuilds derived state.\nsource`; hash `b3209c440805716a35f247dbecf963207fd2e7b102964a72bd6198251be5951d`. Хеши вычислены стандартным Python hashlib из указанных literal bytes при планировании, не парсером Letopis. Пример основных assertions:
```rust
assert_eq!((state.knowledge.len(), state.sources.len(), state.edges.len()), (3, 2, 5));
assert_eq!(state.sources["src/cache.rs"].created_at, 10);
assert_eq!(state.sources["src/cache.rs#L2-L2"].created_at, 20);
assert_eq!(state.knowledge["knowledge/cache.md"].search_hash.to_hex(),
    "b3209c440805716a35f247dbecf963207fd2e7b102964a72bd6198251be5951d");
```

- [x] **Step 2: Добавить тесты `binary_load_projects_sources_and_is_idempotent` и `binary_inspect_retains_knowledge_bytes_separately_from_source_basis`.** Первый вызывает load без config, проверяет полный набор ID/edges и literals выше; повторный load сохраняет revision и проекции. Второй делает inspect `knowledge/cache.md` + `knowledge/flow.md#reuse`, проверяет корректные строки/хеши и `git show <snapshot>:<path>`, затем меняет live Markdown и ещё раз читает старый snapshot. Source basis равен fixture source tree и содержит `src/cache.rs`; tree из knowledge inspect не считается source basis. Capture user HEAD/index/config и corpus bytes до операций, сравнить после, исключая разрешённые `refs/letopis/snapshots/*`.
- [x] **Step 3: Реализовать только test helpers и inputs; выполнить тесты.**

Run: `cargo test --locked --offline --test thin_slice binary_ -- --nocapture --test-threads=1`.
Expected: 2 scenario tests PASS. На существующем исправном продукте первый запуск может сразу пройти: честно отметить characterization GREEN. Ошибка отсутствующего helper/компиляции не считается behavioral RED. Если обнаружен production defect, сначала сохранить воспроизводимый runtime failure, затем отдельный минимальный fix под TDD; согласованный контракт не менять.

- [x] **Step 4: Проверить чувствительность новых assertions.** В disposable copy изменить `SourceMetadata { ... created_at, ... }` в `src/service/projection.rs` на `created_at: created_at.saturating_add(1)`; тест source counts/metadata обязан скомпилироваться и упасть по timestamp. Для snapshot test независимо заставить записываемый blob содержать изменённые bytes, сохранив metadata исходного capture: observable comparison с Git bytes обязан упасть. Восстановить точные bytes, повторить те же тесты до GREEN. Не коммитить controls и не запускать cargo-mutants.
- [x] **Step 5: Передать loader пользователю TK-15.** В `tests/cli/README.md` добавить воспроизводимую последовательность: новый disposable Git repo → source commit/tree → materialize templates → вызов бинаря search/inspect. Команды только с argv/quoted paths; loader не пишет SQL и не требует модели. Объяснить, как будущий TK-15 генератор заменит только набор входных файлов. Через такой же production refresh допускается масштабная загрузка; её скорость здесь не обещается.
- [x] **Step 6: Включить новые inputs в runner inventory и Linux context.** Существующие recursive inventory и Docker COPY `tests/fixtures/markdown` уже включают подкаталог `thin_slice`; менять эти правила не требуется. `test_inventory_covers_support_schema_and_ignores_bytecode` проверяет все три fixture-файла и test/helper. `cargo test --test thin_slice` в Linux должен видеть те же bytes. Публикация skill будет отдельно включена в inventory/copy Task 3.
- [x] **Step 7: Проверить focused suite и commit только paths Task 1.**

Run: `cargo test --locked --offline --test thin_slice -- --test-threads=1` и `python3 -B -m unittest discover -s tests/cli -p test_runner.py` в подготовленном Python env.
Expected: PASS. Commit: `test: add reproducible CLI thin-slice loading and source projection`.

## Task 2: Background Completion, Failure Isolation and Receipts

**Files:** `tests/thin_slice.rs`, `tests/support/thin_slice.rs`, `tests/cli/run.py`, `tests/cli/test_runner.py`, `tests/cli/README.md`. Product paths — только при подтверждённом дефекте, отдельным узким fix с обновлением плана.

**Interfaces — consumes:** Task 1 `SliceFixture`, `ProjectionSnapshot`, marker `THIN_SLICE_RESPONSE`. Существующие `HttpFixture::start(FixtureReply::{Status,Barrier})`, `ResponseBarrier::{wait_until_entered,release}`, `HttpFixture::{get_base_url,take_requests}`, `ReadingRepo::write_config`, `cli::OwnedChild`, `drain::DetachedOwner::{new,wait_for_exit}`. `DetachedOwner` получает PID конкретного foreground, использует ownership checks только в тестах.

**Interfaces — produces:** `capture_thin_slice_responses(evidence: Path, catalogue: dict) -> None` в `tests/cli/run.py`; читает `thin-slice.log`, schema-validates каждый actual CLI envelope через существующий `validate_response`, сохраняет `thin-slice-responses.json`. Обязательные cases: `load`, `inspect`, `background`, `provider_failure`, `invalid_neighbor`, `invalid_inspect`, `text_race`.

- [x] **Step 1: Добавить `binary_search_returns_before_background_embedding_and_reuses_vectors`.** Загрузить fixture без config, затем записать config с local barrier HTTP и запустить настоящий search как owned child. По HTTP checkpoint и завершению foreground доказать независимость; отпустить HTTP, дождаться принадлежащего тесту detached child. Assert ровно 3 document requests, cache search input равен literal Task 1, current vector по literal hash равен `[1.0,0.0]`. Повторный `embed --drain` с EOF не создаёт новых HTTP запросов и не меняет graph revision/corpus. Query embedding при последующем search может делать отдельный запрос: не смешивать его с document count.
- [x] **Step 2: Добавить `binary_provider_failure_preserves_fts_and_bounds_attempts`.** После config с max_retries=0 и HTTP 503 foreground FTS успешен, detached/direct drain исчерпывает одну попытку на каждый из трёх текстов. Assert stored failures attempts=1, current vectors отсутствуют, corpus и проекции сохранены; следующий FTS успешен без новых document HTTP. Provider body содержит sentinel; stdout/stderr/безопасные persisted errors его не содержат. Повторный прямой drain не возобновляет исчерпанные попытки автоматически.
- [x] **Step 3: Добавить `binary_invalid_file_keeps_last_projection_and_publishes_valid_neighbor`.** После Task 1 fixture испортить JSON metadata в cache.md, валидно изменить flow.md. Новый CLI search публикует валидного соседа, оставляет старую cache projection; `files.valid=false` и safe error зафиксированы. Current inspect cache.md возвращает validation_failed без result; старый inspect tree всё ещё читается. Восстановление cache.md устраняет ошибку при следующем CLI refresh. Assert точные ID/edge membership, а не только exit 0.
- [x] **Step 4: Добавить `binary_text_edit_during_document_http_requires_its_own_vector`.** Запустить drain с barrier, дождаться cache request, изменить только cache text и вызвать реальный CLI search для публикации нового hash, пока старый запрос удерживается. До release vector нового hash отсутствует. После release и завершения owner: отдельные запросы для old/new cache text, current new vector, старый orphan удалён, FTS отражает новую правку; исходники letopis не перезаписаны. Нельзя использовать `Fixture::publish`, так как он вызывает library refresh вместо CLI.
- [x] **Step 5: Свежо выполнить существующие доказательства rollback/crash.**

Run separately, serially:
```sh
cargo test --locked --offline --lib service::document_refresher::tests::process::sigkill_keeps_exact_old_or_complete_new_publication -- --exact --nocapture
cargo test --locked --offline --lib service::document_refresher::tests::process::late_storage_error_rolls_back_every_cache_mutation -- --exact --nocapture
```
Expected: оба PASS на текущем source. Эти тесты используют private process driver реального сервиса, не actual CLI; отчёт явно сохраняет эту границу. Вместе с CLI invalid/race scenarios они закрывают failure consistency; не называть это power-loss drill.
- [x] **Step 6: Выполнить новые tests и controls.**

Run: `cargo test --locked --offline --test thin_slice -- --nocapture --test-threads=1`.
Expected: все 6 TK-14 scenario tests PASS. Для нового background/vector test в disposable source после валидации `compute_vector` возвращает vector с инвертированными значениями: тест обязан скомпилироваться и провалить exact vector assertion. Для failure test изменить только `EmbeddingConfig::get_max_attempts` в `src/service/embedding_config.rs`: вернуть `self.max_attempts.saturating_add(1)` вместо `self.max_attempts`. Тест должен завершиться и выявить attempts=2 вместо 1 и лишние requests; полный timeout не является ожидаемым RED. Для invalid-neighbor test в `projection::publish` передать в `unit.replace_text` пустой срез `&text.search_text[..0]` вместо полного текста; FTS assertion должна упасть при компилируемом источнике. Для text race test в `EmbeddingService::prune` заменить вызов `unit.prune_unreferenced().map_err(map_storage)?` на `let _ = &mut unit`: assertion об отсутствии old orphan vector должна упасть. Каждая правка должна быть узкой, компилируемой и иметь exact restore → same GREEN; при невозможности безопасно изолировать fault выбрать другую семантическую поломку того же observable результата и записать её.
- [x] **Step 7: Расширить существующий runner, не создавать второй.** Добавить отдельный запуск `cargo test --locked --offline --test thin_slice -- --nocapture --test-threads=1` в `run_host`, затем `capture_thin_slice_responses`. Validate case/command/exit mapping: load/background/provider_failure/invalid_neighbor/text_race → search/0; inspect → inspect/0; invalid_inspect → inspect/1/validation_failed. Missing case, schema-invalid record и неправильный exit обязаны отклонять receipt. Marker показывает фактический вызов product, не injected handler response.
- [x] **Step 8: Проверить runner tests и commit.** Добавить Python tests `test_thin_slice_requires_all_actual_cases`, `test_thin_slice_rejects_invalid_envelope_or_exit`; каждый удаляет запись/портит envelope и проверяет failure. Run prepared Python `-B -m unittest discover -s tests/cli -p test_runner.py`; Rust focused suite PASS. Commit: `test: cover thin-slice background failures and consistent recovery`.

## Task 3: Agent Skill, Fresh Sessions and Final Acceptance

**Files:** `skills/letopis/SKILL.md`, `tests/cli/{run.py,test_runner.py,Dockerfile,README.md}`, `.dockerignore`, TK-14 card/spec/plan/report. Сценарные prompts/verdicts/raw outputs — в новом `.artifacts/tk14/agent-skill-*`, без дополнительного product harness.

**Interfaces — consumes:** Task 1 fixture layout/loader и public JSON commands; Task 2 validated actual product receipts. `letopis search` stdin `{"query":"CacheScope","limit":10}`, `inspect` stdin `{"element_ids":["knowledge/cache.md"]}`, `embed --drain` с EOF. Search/inspect возвращают links/metadata; bytes читает агент через Git snapshot.

**Interfaces — produces:** переносимый reference skill (name `letopis`, description начинается `Use when...` и описывает triggers), воспроизводимая инструкция TK-15 и acceptance report по всем пяти критериям карточки.

- [ ] **Step 1: Baseline навыка до его написания.** Через superpowers:writing-skills/test-driven-development подготовить одинаковые prompts/rubric, два независимых disposable clone: writer изучает committed src/cache.rs и сохраняет решение + external link; reader находит и читает сохранённое решение из новой сессии. До SKILL.md один свежий агент получает только prompt, fixture root и binary path, без этой спецификации/плана. Записать реальные нарушения grammar/metadata/snapshot/CLI availability. Не выдавать compile failure или уже корректный baseline за behavioral RED; не создавать искусственную ошибку ради отчёта. Если baseline уже корректен, записать этот факт и проверить второй реалистичный pressure scenario; отсутствие RED отметить как ограничение процесса.
- [ ] **Step 2: Написать `skills/letopis/SKILL.md`.** До 250–300 строк, inline рабочие JSON/X-4 примеры вместо копии дизайна. Порядок: поиск существующего решения → проверка source bytes/tree → запись whole-file или явного anchor → CLI refresh → inspect knowledge snapshot и чтение bytes → embedding drain/retry. created_at сохраняется при правке; includes не является вложенной разметкой. Явно различить source_snapshot основания и knowledge snapshot inspect. Не выставлять default/max model dimensions и не инициировать provider install. Таблица: search, knowledge inspect, embed --drain доступны; check/traverse/status/full source inspect/filters пока недоступны с соответствующими task IDs. Ошибки не скрываются; для check описать будущий контракт без успешного вымышленного примера. Никакой глобальной установки skill и правок пользовательских agent instructions.
- [ ] **Step 3: Проверить навык настоящими свежими агентами.** Writer получает только skill + Task 1 source/CLI fixture и сохраняет новую запись; reader с чистым контекстом получает тот же skill + repo/binary, не writer transcript и не ожидаемый ответ. Reader обязан через CLI обнаружить запись, получить inspect и прочесть Git bytes, назвать решение и источник. Controller независимо проверяет metadata/ссылки/source tree/counts и валидирует actual JSON receipts. Provider configuration заранее указывает только controlled loopback; не добавлять агентам API secrets/план реализации. Это smoke workflow, не usefulness pilot и не оценка качества памяти. Корректировки skill отвечают наблюдаемым ошибкам; повторяется только затронутый сценарий.
- [ ] **Step 4: Включить skill в frozen evidence inventory.** `hash_inputs` включает `skills/letopis/SKILL.md`, Dockerfile копирует этот файл, `.dockerignore` разрешает только нужный путь. Python inventory test проверяет его включение; checksum frozen skill соответствует версии, выданной свежим агентам. Проверить links и front matter; успешный skill smoke сам по себе не подтверждает auto-discovery/установку у всех runtimes.
- [ ] **Step 5: Выполнить итоговые host/Linux gates на одном неизменном source.**

До checks подготовить locked Cargo dependencies (`cargo fetch --locked`, только если отсутствуют) и Python 3.11+ env с jsonschema==4.26.0. Проверки зависимостей не скачивают. Один владелец запускает gates последовательно, лог в файл, без параллельных моделей-наблюдателей и частого polling.

```sh
.artifacts/tk14/python-env/bin/python -B -m unittest discover -s tests/cli -p test_runner.py
.artifacts/tk14/python-env/bin/python -B tests/cli/run.py --platform host --evidence .artifacts/tk14/host-01
.artifacts/tk14/python-env/bin/python -B tests/cli/run.py --platform linux --evidence .artifacts/tk14/linux-01
```

Runner включает `make verify V=1` (fmt/all-target check/Clippy/root debug/Rustdoc/root release), затем focused CLI/drain/thin-slice receipts. Не добавлять отдельный повтор `make verify` после неизменного успешного runner. Host/Linux manifests должны иметь равные input hashes, включая templates и skill. Linux runtime `--network none`, HTTP только loopback; build prep может подготовить зависимости. При отсутствии Docker — оставить Linux явно unrun, не объявлять задачу полностью проверенной на Linux. Storage/native runner не перезапускать без изменений storage/native/build/vendored inputs; если такой delta появился, добавить соответствующий host/Linux gate и отдельное evidence до завершения.
- [ ] **Step 6: Итоговое независимое ревью и отчёт.** Проверить actual changed files, visibility, import direction, trait implementations, lib wiring и все критерии. В Native — один свежий whole-branch reviewer после gates; в Subagent-driven — per-task gates + whole-branch review согласно выбранному workflow. После исправления findings повторить только затронутые tests и необходимые final gates на новом source, не приписывать старые receipts новому дереву.

Отчёт `docs/zkit/letopis/checks/TK-14/report.md`: точные команды/exit/platform/hash; реальные counts без сложения nested helper summaries; новые controls по именам со compiled RED/restore/GREEN; отдельный fresh-agent smoke; explicit subprocess-vs-CLI crash boundary; accepted decisions/отклонения/непроверенное. Отметить пять criteria карточки только при наличии доказательств. У skill, roadmap card и spec/plan согласованные статусы; updated accepted facts сохранить в существующем Engram task с сохранением всех attrs. Commit: `feat: document and verify the initial Letopis agent workflow`.

## Author Self-Review and Handoff

- Coverage: пять критериев TK-14 → Task 1 loading/source/snapshot, Task 2 failures/drain/crash receipts, Task 3 skill/new-session/platform/report.
- Type/signature scan: интерфейсы test-only; service names взяты из текущего ae7d2eb. Runtime/Git/Storage interfaces не расширяются. Новые helpers не становятся product API.
- Review Focus 1–2 → Task 1 Steps 1–4; 3–4 → Task 2 Steps 1–3; 5 → Task 2 Step 4 и Task 3 Steps 1–3.
- Existing behavior может изначально пройти новые characterization tests; это не заявляется как pre-implementation RED. Реальные новые defects следуют TDD; test sensitivity доказывается compiling controls.
- План не заявляет будущие commands/полный source lifecycle/RAM/real-model quality. Межзадачные dependencies линейны, общие helper/test/runner files пересекаются.
- Рекомендация автора для этого плана: **Native**, затем один независимый итоговый review. Это уменьшает повторную передачу контекста для трёх тесно связанных задач; выбор остаётся пользователю.

Пользователь утвердил спецификацию и этот план; Native выбран ответом «Хорошо.» на предложение плана и этого метода. Реализация выполняется. Merge/push/issue closure не входят в текущее утверждение плана и выполняются по пользовательской команде после приёмки.
