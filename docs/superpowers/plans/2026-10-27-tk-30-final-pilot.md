# TK-30 Final Pilot Preparation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Подготовить девять перспективных coding-задач, независимые старты и
исполняемую локальную проверку готовности финального пилота без модельных запусков.

**Architecture:** Операторские Python-инструменты подготавливают копии и проверяют
их отдельно от продукта. Существующий P-1 остаётся владельцем формата наблюдений;
локальный preparation manifest описывает незаполненные inputs и не является
состоявшимся study. Никаких изменений product Rust или решений девяти заданий.

**Tech Stack:** Python 3.11+ standard library, Git argv, существующие pinned
Rust/Cargo/SQLite/vec1, model-free Docker fixtures, Codex CLI 0.164.0 как проверяемый
локальный executable; без inference, auth, новых Python/Rust dependencies.

**Spec:** [Утверждённая спецификация](../specs/2026-10-27-tk-30-final-pilot-design.md).
План подготовлен 2026-10-27; **утверждён пользователем 2026-10-27** вместе со
спецификацией, исполнение выбрано Subagent-driven; шаги отмечаются по факту выполнения.

## Global Constraints

- **3 цепочки × 3 задачи × 2 условия = 18 новых сессий** в будущем; сейчас 0.
- Цепочки: C1 CLI #53; C2 частичная локальная поставка #43; C3 частичный bounded
  drain #49 с **границей 2**, blocking I/O/`std`, без async, batching/config knob.
- Эти feature changes — задания будущему участнику, не код реализации TK-30.
  Их экспериментальные патчи не вливаются и не закрывают родительские issues.
- TK-29/G-2 и ограничения TK-16/TK-17/G-1 сохраняются. Текущая preparation base
  `d260348619c6f4f623f24b907ca5bf41fb14eba9` не является финальной post-TK-29 сборкой.
- **Codex CLI `0.164.0`**, отдельные agent session/HOME/CODEX_HOME на каждый слот,
  новый пустой корпус каждой memory-цепочки. Нет наследования Engram/истории чата.
- Бинарник памяти/skill read-only и отделены от mutable coding binary, включая C3.
  FTS-only smoke не объявляется готовностью полного embedding-профиля.
- Python **3.11+**, P-1 **v1**; unknown usage — **null**, не zero. Cached/reasoning
  входят в input/output. Нет target improvement, денежного/каузального вывода.
- Все evidence directories новые; существующие результаты/другие worktrees не
  перезаписываются. Исходники TK-17 читаются только по полному commit
  `a53ba2cc7fe2803c564d54ac89f777aee61bfd56`, без merge его ветки.
- Product `src/`, Cargo, schema, `skills/letopis/SKILL.md`, P-1 checker и TK-29
  принадлежат другим владельцам. Новые вспомогательные Rust-файлы — только oracle
  source оператора, компилируемый в disposable evaluation copy.
- Каждый новый тест: Given/When/Then, независимые literal ожидания, ручной
  semantic mutation с восстановлением. Компактные логи; 40/60 строк на функцию.
- Текущая реализация подготовки не содержит команды `session` или вызова
  `codex exec`; проверка версии/help допускается, credentials не читаются.

## Review Focus

1. Ветка может сдвинуться, задача — уже оказаться решённой: `test_base_drift_keeps_unready`
   и RED независимого нового критерия на exact seed — Tasks 1/3/5.
2. Через symlink/старый Git object/target может просочиться чужой ответ:
   `test_seed_rejects_escape`, `test_evaluator_cannot_write_back` — Task 2.
3. Однопоточный HTTP fixture выдаёт ложный RED параллельности:
   `test_overlap_observer_counts_two_connections` — Task 3, настоящий concurrent server.
4. Равное число unrelated telemetry events не доказывает полный usage:
   `test_equal_counts_without_correlation_remain_unknown` — Task 4.
5. Unknown configuration или локальный smoke ошибочно превращаются в ready:
   `test_missing_product_model_or_gate_stays_unready`,
   `test_prepare_never_reads_auth_or_executes_model` — Tasks 1/4/5.

## File ownership and interfaces

Создавать файл только в задаче, которая реализует его ответственность; не
создавать пустые modules. Файлы ниже — будущие, сейчас их нет.

| Файлы | Ответственность |
| --- | --- |
| `docs/zkit/letopis/pilot/final/selection.json` | Один источник 3 цепочек/9 prompts/check IDs/reuse_from; только prospectively selected inputs |
| `docs/zkit/letopis/pilot/final/instructions.md`, `readiness.md` | Общие/condition instructions и явный freeze checklist; не содержат решений |
| `tests/final_pilot/study.py`, `run.py` | Проверка selection, manifest, order; только model-free CLI prepare/audit/evaluate |
| `tests/final_pilot/copies.py` | Inventories, независимые seeds, capture и одноразовый evaluator; file modes отдельно от P-1 byte hash |
| `tests/final_pilot/evaluation.py`, `oracles/cli.py`, `oracles/delivery.py`, `oracles/drain.py`, `oracles/service_probe.rs` | Dispatch и независимые наблюдения девяти задач; код оператора исключён из participant |
| `tests/final_pilot/environment.py`, `telemetry.py`, `records.py` | Проверки image/tools и нормализация supplied logs/templates без запуска агента |
| `tests/final_pilot/test_study.py`, `test_copies.py`, `test_oracles.py`, `test_environment.py`, `test_records.py` | Focused tests соответствующих границ |
| `tests/final_pilot/README.md`, `docs/zkit/letopis/checks/TK-30/report.md`, `verification.json` | Команды, текущие доказательства/пределы и передача следующему владельцу |

Existing references: [P-1](../../zkit/letopis/pilot/protocol.md),
[checker README](../../../tests/pilot_protocol/README.md),
`tests/embedding_service.rs`, `tests/support/drain.rs`, `src/lib.rs`.
Названия и поля ниже — общие контракты задач плана, не новое API Letopis.
Частные helpers/точный расклад небольших unittest classes остаются исполнителю.

### Task 1: Freeze prospective definitions and explicit readiness

**Files:** Create `selection.json`, `instructions.md`, `readiness.md`,
`tests/final_pilot/study.py`, `run.py`, `test_study.py`.

**Interfaces:**
- `study.load_selection(path: Path) -> dict`: строго 3 × 3 unique task IDs,
  previous-task-only `reuse_from`, prompts/check IDs/agreement IDs и issue sources.
- `study.build_manifest(selection: dict, inputs: dict) -> dict`: private
  `preparation_version=1`, `state="unready"`, `model_executions=0`,
  `original_base`, nullable `seed_commit`, `starting_inventory`, `product`,
  `environment`, `agent`, `gates`, `pending`, `order`, `source_receipts`.
- `study.assess_readiness(manifest: dict) -> dict`: `{state, pending}`;
  никогда не выводит доступ к модели или gate verdict из отсутствия ошибки.
- `run.main(argv: list[str]) -> int`: `prepare --repo PATH --base FULL_OID
  --output NEW_DIR`, `audit --root DIR`, `evaluate --root DIR --slot SLOT`.
  JSON stdout; exit 0 — успешная локальная операция, exit 2 — неверные inputs/infra.
  `audit` с pending возвращает state unready и exit 2. Нет paid/session subcommand.

- [x] **1. Написать failing tests.** В `test_study.py` проверить exact order:
  C1 baseline T1/T2/T3, затем memory T1/T2/T3; C2 memory перед baseline;
  C3 baseline перед memory. Отсутствующая T3, repeated ID, forward reuse и
  already-existing output directory отклоняются. `test_missing_product_model_or_gate_stays_unready`
  требует pending для null model/config/product/G-2 даже при `local_tools_ok=true`.
  `test_base_drift_keeps_unready`: изменение проверенного commit/selection hash
  инвалидирует прошлый freeze, не переносит `ready`.

```python
# Then: literal contract; no generated oracle for expected slot count.
self.assertEqual(len(manifest['order']), 18)
self.assertEqual(manifest['model_executions'], 0)
self.assertEqual(readiness['state'], 'unready')
self.assertIn('g2', readiness['pending'])
```

- [x] **2. Увидеть RED.** `python3 -B -m unittest discover -s tests/final_pilot
  -p test_study.py`; сначала FAIL отсутствующего implementation, затем нужного
  контракта. Не принимать ошибку импорта как доказательство semantic control.
- [x] **3. Создать definitions и минимальные функции.** Полные prompts из
  утверждённой spec, не указания решения. C1 fixed expected version берётся из
  frozen Cargo.toml (сейчас literal `0.0.0`), перечень команд — из frozen X-1.
  C2 уточнить исполнимый контракт прямо в prompts:
  `python3 scripts/package.py --binary PATH --skill PATH --output NEW_DIR`,
  `python3 scripts/install.py --bundle DIR --prefix DIR`,
  `python3 scripts/check_install.py --prefix DIR`. Bundle: `bin/letopis`,
  `share/letopis/skills/letopis/SKILL.md`, `manifest.json` с `version`,
  `source_commit`, `target`, sorted `files[{path,sha256}]`. Installer принимает
  те же относительные пути; exit 0 — успех, nonzero — ошибка. Запретить overwrite
  коллизии/повреждения и egress. Это контракты будущих заданий, не новые scripts.
  C3 граница 2 без runtime/config расширения. Зафиксировать `hexagonal_sync`
  и `typed_safe_errors` как оцениваемые agreements и конкретные recurring facts
  по каждой цепочке; hidden scoring rubric не попадает в participant.
- [x] **4. GREEN и controls.** Повторить только study suite. Временно разрешить
  forward reuse, убрать один required pending — каждый соответствующий тест
  обязан упасть; восстановить. Отдельно `run.py session` с trap auth path должен
  exit 2 до любых чтений auth/старта executable. Не создавать P-1 session records.
- [x] **5. Коммит.** Только перечисленные пути:
  `test: define TK-30 prospective tasks and preparation manifest`.

### Task 2: Build isolated paired seeds and disposable evaluation copies

**Files:** Create `copies.py`, `test_copies.py`; integrate real prepare/audit in `run.py`.

**Interfaces:**
- `copies.snapshot(root: Path, exclusions: tuple[str, ...]) -> dict` returns
  P-1 `{items, sha256, inventory}`; canonical UTF-8 JSON rows + LF exactly as P-1.
- `copies.prepare_seeds(repo: Path, base: str, output: Path) -> dict` creates
  six `conditions/{C1,C2,C3}-{baseline,memory}/repo` with independent `.git`;
  returns original/seed commits, inventories, exclusions and Git receipts.
- `copies.capture_slot(root: Path, slot: str) -> dict` captures exact current
  code/corpus and full participant audit inventory before any evaluator.
- `copies.prepare_evaluation(root: Path, slot: str) -> Path` reconstructs only
  captured bytes/modes in new `operator/evaluation/SLOT`, own `.git`/target/HOME.
- `copies.require_unchanged(root: Path, before: dict) -> None` compares full
  participant bytes/modes including `.git`, target and per-slot HOME.

- [x] **1. Написать failing tests.** Pair inventories равны; шесть Git dirs
  не shared/alternate/remote, исходный repository не меняется. Snapshot empty
  hash — literal `37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570`
  для `[]\n` (перед внесением сверить независимым SHA-256). Symlink escape,
  parent path, чужой root и изменение между inventory/capture отклоняются.
  `test_capture_precedes_evaluation` возвращает в T2 собственный captured T1,
  а не изменённую evaluation copy/peer condition. Новые HOME у каждого слота.
- [x] **2. Увидеть RED** focused `test_copies.py`; зафиксировать конкретное
  ожидание, не считать отсутствие Docker валидным failure isolation test.
- [x] **3. Реализовать копирование.** Перенести/адаптировать лишь snapshot,
  preserve/restore и isolation checks из pinned TK-17 `evidence.py`/`copies.py`;
  сохранить перечень исходных файлов+hash в `source_receipts`. Git archive только
  полного OID; явные одинаковые exclusions: host state/build outputs/operator
  pilot code/docs/evidence. Записывать original→sanitized seed mapping и diff
  общих AGENTS instructions: сохранять design/style, заменить только Engram/
  host workflow на одинаковый pilot instruction. Fixture Markdown остаётся
  физическим test input, но исключён из индексируемого Git inventory одинаково.
  Не задавать агенту обязательный каталог новых знаний; фиксировать фактический
  inventory записанного корпуса. Отдельные байты/Unix modes сохраняются для
  исполнимых scripts; P-1 hash по-прежнему только canonical byte inventory.
- [x] **4. GREEN и настоящий boundary proof.** Unittest + model-free Docker
  toy build.rs, читающий private marker и пишущий в evaluation copy. T2 mounts
  и оригинальные code/Git/target/HOME marker не видят; unsafe shared-mount control
  обязан обнаруживаться. `copies.require_unchanged` ловит удалённый audit. Нет
  обхода через hard links, host mounts или прежний cargo cache с oracle binaries.
  Останавливать только контейнер с owned run ID. Невозможный Docker proof —
  pending, не PASS; никакой модели для него не нужно.
- [x] **5. Коммит.** `test: isolate TK-30 seeds and evaluator state`.

### Task 3: Implement independent acceptance oracles, not task solutions

**Files:** Create `evaluation.py`, `oracles/{cli,delivery,drain}.py`,
`oracles/service_probe.rs`, `test_oracles.py`; fill selection acceptance references.

**Interfaces:**
- `evaluation.evaluate(root: Path, slot: str) -> dict` calls Task 2 capture/copy,
  dispatches exactly the slot's oracle and returns private
  `{task_id, status, checks, execution}`, where status is `assessed|infra_error`,
  each check `{id, passed: bool | None, evidence}`. Infra/timeout is never a
  successful or ordinary negative feature assertion. Preserve exits/elapsed
  before grading; call `require_unchanged` even after failure/interruption.
  Для явно ручных критериев читает отдельный operator annotation с ref на
  captured source; без него `passed=null`, не автоматический PASS.
- Each oracle module implements `evaluate(copy: Path, task_id: str, evidence: Path)
  -> list[dict]`; check IDs below are frozen in selection. Common regression
  check runs `make check` in that disposable source, compact success only.
- Private service probe uses existing `open_storage`, `wire_embedder` and
  `Embedder::drain`; copied to evaluation `examples/tk30_service_probe.rs` and
  compiled offline there. No product exposure or source-side test hook.

- [x] **1. Написать failing tests** для dispatch неизвестного ID, несоответствия
  expected check IDs и preservation infra failures. Oracle unit inputs — literal
  filesystem/HTTP/JSON fixtures, не готовые реализации C1–C3 и не fake pilot records.
  Expected output не строится parser/normalizer участника.
  `test_buffer_pass_requires_source_audit`: два barrier requests без annotation
  дают `bounded_buffer.passed=null`; ссылка на другой captured commit отклоняется.
- [x] **2. Реализовать CLI/delivery наблюдения.** Для C1 open stdin остаётся
  незакрытым; flags обязаны завершиться до watchdog, с пустым HOME и вне repo.
  Compare exact `letopis 0.0.0\n` для current frozen package version; новая версия
  меняет frozen expected при новом выборе, не извлекается из ответа программы.
  Wrong argv сохраняет прежнюю JSON failure. Help проверяет семантические поля/
  примеры каждого frozen command, не whitespace/порядок абзацев. C2 проверяет
  exact layout/hash/modes и source manifest; checksum oracle — hashlib над
  заранее известными bytes. Сценарии повреждения/коллизии сохраняют исходный
  inventory. Installed smoke запускает абсолютный installed binary из fresh
  temporary repo с literal note/source и без embeddings config; source tree/target
  недоступны. Это smoke проверки поставки, не полный memory pilot profile.
- [x] **3. Реализовать concurrent drain наблюдения.** Python
  `ThreadingHTTPServer` принимает запросы независимо; barrier подтверждает
  **два принятых и удерживаемых запроса до release первого**, третий остаётся
  не отправлен. `test_overlap_observer_counts_two_connections` использует две
  реальные sockets, проверяет счётчик 2, а sequential toy control — 1. Не
  использовать serial `HttpFixture` из `tests/support/embedding.rs` как oracle
  параллельности. Service probe публикует literal distinct/shared hashes,
  читает реальные attempts до ответа и vectors после него; отдельные изменения
  текста/config и ответы в обратном порядке задаёт оператор. На HTTP барьере
  независимая short writer транзакция завершается, подтверждая отсутствие
  транзакции вокруг inference. T3 использует настоящий binary, pid/lock
  acknowledgements, foreground SIGINT и завершение собственного drain, без
  убийства посторонних процессов. `bounded_buffer` сочетает barrier-наблюдение
  двух HTTP requests и ручной аудит captured allocation/queue paths: фиксированный
  window/queue capacity **2**, отсутствие eager whole-corpus text/result collection.
  Сохранить symbol/line refs и независимый вывод оператора; без аудита null.
  Это проверка кода и конкретного interleaving, не количественный whole-stack
  RAM verdict. Одно число active HTTP не доказывает buffer bound; не вводить
  production hooks, универсальный allocation profiler или новый provider API.

| Task | Frozen check IDs (дополнительно `regression`) |
| --- | --- |
| C1-T1 | `version_value`, `version_no_stdin_or_state` |
| C1-T2 | `help_commands_and_examples`, `help_no_stdin_or_state` |
| C1-T3 | `command_help_forms`, `help_rejects_invalid_argv` |
| C2-T1 | `bundle_layout`, `manifest_integrity`, `bundle_rejects_invalid_input` |
| C2-T2 | `install_layout_and_repeat`, `install_failure_preserves_previous` |
| C2-T3 | `installed_workflow`, `installed_missing_or_damaged`, `installed_independence` |
| C3-T1 | `two_requests_before_release`, `bounded_buffer`, `one_owner_short_writes` |
| C3-T2 | `shared_hash_durable_attempts`, `out_of_order_identity`, `invalid_retry_bound` |
| C3-T3 | `detached_overlap`, `caller_owner_lifecycle`, `search_with_drain` |

- [x] **4. Проверить seed и oracle controls.** Для каждого из 9 tasks сохранить
  хотя бы один настоящий RED нового критерия на unchanged seed; прежние regression
  checks могут быть GREEN. Если новый criterion уже GREEN, selection остаётся
  unready до пересмотра, не удалять готовое поведение из seed. Отсутствующий
  `scripts/package.py` — отсутствующая feature, но failed Cargo build/неверный PATH
  — infra_error. Compile service probe отдельно от запуска, не принимать compile
  failure за RED concurrency. Mutation controls: неверная версия/help, повреждённый
  bundle, overwrite collision, swapped vectors/identity, duplicate durable attempt,
  changed threshold 2→3 и bypass participant audit; каждый соответствующий
  operator test обязан упасть. Controls — временные mutations oracle/fixture,
  не написание девяти корректных product implementations.
- [x] **5. Коммит.** `test: prepare independent TK-30 task acceptance oracles`.

### Task 4: Prepare credential-free environment and honest observation records

**Files:** Create `environment.py`, `telemetry.py`, `records.py`,
`test_environment.py`, `test_records.py`; integrate preflight/audit outputs.

**Interfaces:**
- `environment.prepare_context(source: Path, assets: Path, output: Path) -> dict`
  creates new credential-free context from supplied pinned assets; no implicit
  download/auth/image execution. Store hashes/digest; input asset mismatch fails.
- `environment.preflight(root: Path, image: str) -> dict` runs only local tool/
  access/memory initialization probes and returns `{checks, pending, model_executions}`.
- `telemetry.normalize_events(events: list[dict], context: dict) -> dict` returns
  independent `{request_inventory, requests, complete, unresolved}`; context fixes
  slot/model/purpose. Explicit actual correlation identity required for pairing.
- `records.build_templates(manifest: dict, output: Path) -> dict` produces 18
  **unexecuted private preparation slots**, checks null, no invented session IDs,
  empty usage and state unready; these are not submitted as P-1 observations.
- `records.write_demo(output: Path) -> Path` uses existing P-1
  `fixtures.make_dataset(..., stage="final")`, kind demonstration unchanged.
  Supplied real records later enter unchanged `tests/pilot_protocol/check.py`.

- [x] **1. Написать failing tests.** Auth file is a trap; preparing/inspecting
  assets/version/tools cannot open it or invoke `codex exec`/provider. Test null
  model/product/G-2, CLI version mismatch, corrupted asset and stale image digest.
  Count equality with mismatched correlation yields complete=false/null usage.
  Retried failed request and control completion remain rows, not overwritten.

```python
# Then: independently authored input=100, output=20, cached=80, reasoning=5.
self.assertEqual(request['input_tokens'] + request['output_tokens'], 120)
self.assertEqual(request['cached_input_tokens'], 80)
self.assertEqual(request['reasoning_output_tokens'], 5)
self.assertFalse(unmatched['complete'])
```

- [x] **2. Увидеть RED**, затем реализовать только эти функции. Переносимые
  куски TK-17 image/tools/collector читаются по pinned revision и hash; убрать
  его hardcoded seed/study/order, `session`, auth copy и downloader. Не переносить
  count-only `collect_usage` как доказательство correlation. Codex request trace/
  response IDs используются только если реально присутствуют; при отсутствии
  связи полные totals неизвестны. Повторение одного collector event не дублирует
  один request, но отдельный retry имеет отдельный ID. Конфликтующие дубликаты
  остаются unresolved. Unmatched completion сохраняется отдельно, а не теряется.
  Local fixture/tests допустимы; реальный endpoint модели не вызывается.
- [ ] **3. Model-free preflight.** _Частично (Task 4): tools/memory/container_isolation/egress probes и pending реализованы; отдельные Git/target и изоляция участника остаются pending `preflight:git_target_separation`/`participant_isolation` до команды запуска участника (TK-31)._ Проверить image login-shell PATH для
  rustc/cargo/rustfmt/git/rg/Python/Codex 0.164.0 и fixed memory binary, пустой
  memory store по CLI/read-only SQL, отдельные Git/target, canaries peer/operator
  и отсутствие host egress. Инициализировать disposable память **до** записи
  provider config, чтобы check не запустил настоящий drain. Config bytes/provider
  choice сохранять как input, но доступ/реальная embedding telemetry остаются
  pending. По model-free toy endpoint можно проверить isolation route, не inference.
  Context не содержит repo с готовыми решениями/credentials; Docker socket и
  host filesystem не монтируются. Если готовых pinned assets/image нет — честный
  pending с точным недостающим input, без скачивания/расширения scopes.
- [x] **4. GREEN, demonstration и controls.** Запустить environment/records
  unittest. Existing P-1 checker на generated final demo даёт
  `status=demonstration_only`, `agent_executions=0`, все 18 slots присутствуют.
  Unknown counters остаются null и не превращаются в нулевую экономию.
  Временно разрешить auth read, объединить unmatched counters, прибавить cached
  повторно или поставить `ready` при missing config: соответствующие tests RED;
  восстановить. Не создавать `kind=pilot` с демонстрационными данными.
- [x] **5. Коммит.** `test: prepare model-free TK-30 environment and record validation`.

### Task 5: Verify the preparation and hand off explicit blockers

**Files:** Create README/report/verification; update TK-30 card section 9 with
preparation status/evidence, keeping unmet acceptance boxes unchecked; update
spec/plan checkboxes only for фактически выполненные шаги.

**Interfaces:** `run.py audit --root DIR` aggregates Tasks 1–4 receipts without
rerunning product/model work; report maps each TK-30 criterion to evidence/pending.
No aggregate ready verdict while gate/model/final product inputs remain unresolved.

- [x] **1. Выполнить targeted verification один раз на окончательных inputs.** _Выполнено на preparation base `d260348`, а не на финальной post-TK-29 base; повтор на финальной base остаётся частью `base_drift_recheck`._
  `python3 -B -m unittest discover -s tests/final_pilot -p 'test_*.py'` — все
  доступные focused tests PASS, opt-in Docker tests явно перечислены.
  `python3 -B -m unittest discover -s tests/pilot_protocol -p 'test_*.py'` —
  unchanged protocol suite PASS. Docker model-free proof выполнить отдельно на
  новой evidence directory; пропуск не скрывать. Не запускать cargo-mutants,
  исторические native/RAM/load/pilot suites и paid probes. Root `make verify`
  не нужен для operator-only changes; в evaluation copies компилируется private
  probe и выполняется оговорённая regression, не выдавать их за root change gate.
- [x] **2. Записать local preparation/audit results.** Например,
  `python3 -B tests/final_pilot/run.py prepare --repo . --base FULL_OID --output
  .artifacts/tk30/preparation-NEW`, затем `audit --root ...`.
  Expected: созданные inputs/receipts и **state=unready**, exit 2 пока pending;
  это ожидаемое ограничение, не успешный запуск пилота. Отдельно `evaluate` для
  seed сохраняет реальные RED/infra results. Нет девяти task solutions, prompts
  будущих slots не смонтированы, product/skill source неизменны.
- [x] **3. Подготовить отчёт и evidence.** Exact source/tool/image revisions,
  hashes, команды/exit/platform, manual mutations/restoration, критерии RED и
  пределы. Сопоставить G-2 с TK-29 evidence, когда она появится; без неё pending.
  Недоступные final post-TK-29 base, pinned assets, real model access и полный
  usage — explicit blockers с владельцем/следующим шагом, не формальный TK-30 done.
  Архитектура: перечислить operator файлы, подтвердить отсутствие product
  dependencies/нового wiring; private Rust probe не включён в participant inventory.
- [ ] **4. Итоговая проверка и ревью.** Проверить affected links,
  `git diff --check`, source receipts и scope diff. Независимое whole-branch review
  должно проверить отсутствие evaluator write-back, платных call paths и false
  readiness. Исправить существенные findings и повторить только затронутые tests.
  Сохранить утверждённые решения/результаты в существующий Engram store с точными
  optimistic tokens и сохранением старых attrs; чужой dirty graph не трогать.
- [ ] **5. Коммит и handoff.** `docs: report TK-30 preparation and remaining gates`.
  Сдать список готовых материалов, pending inputs/gates и разрешения, нужного
  для будущих TK-31/TK-32. Без merge/push/issue closure и без paid calls.

## Self-review and next step

Coverage: spec §§1–4 → Task 1/3; §§5–6 → Task 2/3; §7 → Tasks 1/4/5;
§8 → Task 4; §9 → Task 5. Все пять Review Focus risks имеют named tests;
общие interfaces/signatures сверены между задачами. Модель/бюджет и реальные
gate/access proofs оставлены pending намеренно, а не заменены предположениями.

Это **план подготовки**, не объявление реализации. Пользователь утвердил
спецификацию и план 2026-10-27 и явно выбрал Subagent-driven Development;
повторное утверждение не требуется. Исполнение: по одному свежему исполнителю на задачу,
ревью задачи после каждой и независимое ревью всей ветки в конце.
