# TK-30 — отчёт о подготовке финального пилота

2026-10-28. **Подготовка, не выполненный пилот и не formal TK-30 done.** Состояние
подготовки — **unready**: `audit` возвращает `state=unready`, exit 2. Платных сессий,
вызовов модели/provider, чтения credentials и `codex exec` не было (`model_executions=0`).
Девять заданий не реализованы; их решения — работа будущих участников TK-31/TK-32.
[Спецификация](../../../../superpowers/specs/2026-10-27-tk-30-final-pilot-design.md),
[план](../../../../superpowers/plans/2026-10-27-tk-30-final-pilot.md),
[карточка](../../roadmap/epics/EP-5-final-pilot/tasks/TK-30-pilot-setup.md),
[operator README](../../../../../tests/final_pilot/README.md),
[readiness checklist](../../pilot/final/readiness.md) и
[машиночитаемый receipt](verification.json) задают scope и provenance.

## Точный source, инструменты, платформа

| Что | Значение |
| --- | --- |
| Operator source | ветка `tk-30`, `73311873254c3399a2fc133d9d45064a2807c7c6` — последний commit, меняющий operator code (final-review fixes: исключение `checks/TK-30…33` из seeds, evidence-gates в `audit`, release lock только по наблюдённому `status`, обнаружение подмены operator state, `--pull never`, Python 3.11.4); хэши operator файлов в receipt — на нём. Unittest-сьюты, `prepare`/`audit` и поиск утечек по шести реальным seeds повторены на `8aba7d0`; 9 seed-слотов, Docker proofs, `context` и `preflight` — на более ранних commits (`4c308cd`, relative-root `evaluate` на `74e0f82`); seed bytes не изменились (тот же seed commit и inventory). Последующие docs commits меняют только docs/receipt |
| Preparation base | `d260348619c6f4f623f24b907ca5bf41fb14eba9` — **не** финальная post-TK-29 сборка |
| TK-17 sources (только `git show`, без merge) | `a53ba2cc7fe2803c564d54ac89f777aee61bfd56`, 8 файлов, все SHA-256 пересчитаны и совпали (см. receipt) |
| Платформа | macOS 26.6.2 (25G83), Darwin 25.6.0, arm64 |
| Инструменты хоста | Python 3.12.0 (`python3` по умолчанию 3.9.6 не используется), rustc/cargo 1.95.0 (pinned `rust-toolchain.toml`), git 2.50.1, Docker 29.9.2 |
| Локальный image (только уже локальный, без pull) | `letopis-tk17-codex:v2-0.164.0` = `sha256:a8b5b012f93d3af8aa1fd2dd0ad4dd1af22a14b1397d224928950b8593c2bbe7` (linux/arm64); исторический TK-17 image, **не** финальный |
| Codex CLI | `codex-cli 0.164.0` — только как вывод `--version` внутри preflight-контейнера |

SHA-256 operator inputs на `8aba7d0`: `selection.json`
`15910f33650b175da044151e58ca9cc58234bda3a8da6f1af6933b08a45f9f06`, `instructions.md`
`518167800673642b6b6f27f8868103d81872c64dd567f30efb66c2578ac02ab7`, `readiness.md`
`f19e875dafb398092f72516521ba478f13f8d39c62c39515fc3df26635aecf12` (изменён: новые evidence-gates; на
`4c308cd`/`74e0f82` был `c40b9fe3…601618`); canonical
`selection_sha256` `84beb7ce6cd4f3c7fb96e71176c7f28877cc214b55769ea6377be81d9a6bb812`.
Хэши всех operator файлов — в receipt.

## Выполненные проверки

Все команды из корня worktree; private evidence — в git-ignored
`.artifacts/tk30/task5-verification-20261028/` (unittest/Docker), `.artifacts/tk30/task5-verification-abs-20261028/` и `.artifacts/tk30/preparation-task5-abs-20261028/`
(не коммитятся).

| Команда | Exit | Результат |
| --- | --- | --- |
| `python3.12 -B -m unittest discover -s tests/final_pilot -p 'test_*.py'` на `8aba7d0` | 0 | `Ran 150 tests in 114.974s OK (skipped=2)`; два skip — opt-in Docker тесты ниже (ранее: 142 на `74e0f82`, 140 на `4c308cd`) |
| на `8aba7d0`: `run.py prepare` в новый scratch root вне checkout, затем `audit` | 0 / 2 | тот же seed `6da9e9a…`, inventory `3229b8f8…`; audit unready, pending теперь включает `preflight`, `participant_isolation`, `correlation_attribute`; поиск всех frozen check/agreement IDs и `tk30_service_probe` по шести seeds — 0 совпадений |
| `python3.12 -B -m unittest discover -s tests/pilot_protocol -p 'test_*.py'` на `74e0f82` | 0 | `Ran 31 tests OK` |
| на `74e0f82`: `run.py prepare --repo . --base d260348… --output .artifacts/tk30/preparation-task5-fix1-rel-20261028`, `audit --root` (относительный), `evaluate --root <тот же относительный> --slot C1-T1-baseline` | 0 / 2 / 0 | тот же seed `6da9e9a…`, inventory `3229b8f8…`; audit unready с теми же 9 pending; evaluate `assessed`, `infra_cause=null`, `cargo build` exit 0, RED `version_value`/`version_no_stdin_or_state`, `regression=true` |
| `python3.12 -B -m unittest discover -s tests/pilot_protocol -p 'test_*.py'` на `4c308cd` (P-1 без изменений) | 0 | `Ran 31 tests OK` |
| `TK30_CONTAINER_OUTPUT=<new> … -p 'test_copies.py' -k ContainerBoundary` | 0 | `Ran 1 test OK`; receipt `PASS`: build-script write-back только в disposable copy, T2 marker не виден, unsafe shared mount обнаружен, owned timeout container удалён |
| `TK30_PREFLIGHT_IMAGE=letopis-tk17-codex:v2-0.164.0 … -p 'test_environment.py' -k LocalImagePreflight` | 0 | `Ran 1 test OK` — тест прошёл, подтвердив **unready** (это не PASS preflight): `image_digest=null` (не закреплён), `tools=false` (`missing:letopis`; image содержит `letopis-slice`), `memory=false`, `container_isolation=true` (только сам preflight-контейнер) |
| `run.py prepare --repo "$PWD" --base d260348… --output "$PWD/.artifacts/tk30/preparation-task5-abs-20261028"` | 0 | `state=unready`; seed commit `6da9e9af718a3a075e3400976257917337b9c394`, 2231 файлов, inventory `3229b8f8…cb0cea`, modes `cbccf91c…c1cc64`; 18 слотов в порядке P-1 |
| `run.py audit --root "$PWD/…/preparation-task5-abs-20261028"` (свежий root) | 2 | `state=unready`; pending `product, memory_versions, embedding, assets, model, agent_config, g2, prompt_freeze, base_drift_recheck`; нет `seed_drift`/`input_drift` — **ожидаемое ограничение, не успех пилота** |
| `run.py evaluate --root "$PWD/…/preparation-task5-abs-20261028" --slot CN-TN-baseline`, 9 слотов последовательно | 0 ×9 | все `assessed`, `infra_cause=null`, 207–261 с на слот; таблица ниже |
| `run.py context --source docs/zkit/letopis/pilot/final --assets <пустой каталог> --output <new>` | 2 | `context=null`, pending `asset:codex-linux.tar.gz`, `asset:code-mode-host.tar.gz`; ничего не скачано и не создано |
| `run.py preflight --root …preparation-task5-abs-20261028 --image letopis-tk17-codex:v2-0.164.0` | 2 | `image_digest=null`, `tools=false`, `memory=false`, `container_isolation=true`; 16 pending, `model_executions=0` |

**Дефект, найденный в Task 5 (исправлен commit `74e0f82`
`test: resolve evaluation root before building`):** первый прогон с относительным
`--root .artifacts/tk30/preparation-task5-20261028` дал для C1-T1 ложный
`infra_error` (`infra_cause=participant_build`), хотя `cargo build` завершился с
exit 0 (`commands.jsonl`). Причина: `evaluation.build_environment` передаёт
`CARGO_TARGET_DIR=str(copy / "target")`; при относительном root cargo разрешает
его относительно `cwd=copy/repo`, а `build_binary` ищет `copy/target/debug/letopis`
относительно cwd оператора. Прогон остановлен после C1-T1; повтор на новом root с
абсолютным путём (тот же seed commit и inventory) дал `assessed` с реальным RED.
Исправление: `run.py` разрешает все path-аргументы в абсолютные, `evaluation.evaluate`
разрешает root; тесты на относительный root и path-аргументы CLI падают без этого
разрешения (ручная мутация). README больше не требует абсолютных путей. Повтор `evaluate --root seed-red-2
--slot C1-T1-baseline` с относительным root (scratchpad, cwd — его родитель) после
исправления дал `assessed`, `infra_cause=null`, RED `version_value`/
`version_no_stdin_or_state` и `regression=true`. Независимый повтор Task 5 на `74e0f82`
со свежим `prepare` в относительный `.artifacts/tk30/preparation-task5-fix1-rel-20261028`
дал тот же результат (строка таблицы выше; receipt `head_74e0f82`).

Не запускались (намеренно, по плану): root `make check`/`make verify` (изменения только
operator Python/docs; product Rust не менялся), cargo-mutants, исторические
native/RAM/load/pilot suites, platform runner storage, платные probes, Linux-прогон
unittest. `make check` выполнялся только как `regression` внутри disposable evaluation
copies — это проверка captured seed, а не root change gate.

## Seed RED на неизменной base

Неизменный seed (`6da9e9a…`, base `d260348`), все `-baseline` слоты, `status=assessed`,
`infra_cause=null`; совпадает с post-fix таблицей Task 3.

| Задача | RED новых критериев | GREEN |
| --- | --- | --- |
| C1-T1 | `version_value`, `version_no_stdin_or_state` (`--version` → JSON `invalid_request`, exit 1) | `regression` |
| C1-T2 | `help_commands_and_examples`, `help_no_stdin_or_state` | `regression` |
| C1-T3 | `command_help_forms`, `help_rejects_invalid_argv` | `regression` |
| C2-T1 | `bundle_layout`, `manifest_integrity`, `bundle_rejects_invalid_input` (`scripts/package.py` нет) | `regression` |
| C2-T2 | `install_layout_and_repeat`, `install_failure_preserves_previous` | `regression` |
| C2-T3 | `installed_workflow`, `installed_missing_or_damaged`, `installed_independence` | `regression` |
| C3-T1 | `two_requests_before_release`, `bounded_buffer` (false: наблюдение не прошло, audit не достигнут), `one_owner_short_writes` | `regression` |
| C3-T2 | `shared_hash_durable_attempts`, `out_of_order_identity`, `invalid_retry_bound` (нет overlap) | `regression` |
| C3-T3 | `detached_overlap` | `caller_owner_lifecycle`, `search_with_drain` (существующее поведение), `regression` |

Критерий RED: каждая задача имеет хотя бы один RED нового критерия на неизменном
seed; GREEN допускается только для прежнего поведения и `regression` (`make check: ok`
в disposable copy). Это base `d260348`; решены ли задачи на финальной post-TK-29 base
— не проверено (`base_drift_recheck`).

## Ручные мутации (из отчётов Tasks 1–4)

Каждая мутация вносилась вручную, названный тест падал, файл восстанавливался и
сверялся побайтно; дерево после каждого раунда чистое. Подробные таблицы — в
отчётах исполнителей (`.superpowers/sdd/2026-10-27-tk-30-final-pilot/task-{1..4}-report.md`,
не входят в seed).

- Task 1 (study/run): forward reuse, удалённые `g2`/`prompt_freeze` из required,
  recheck переживает drift, `local_tools_ok` делает ready, порядок C2, audit доверяет
  stored state, evaluate success, без Python-guard, `session`-подкоманда с trap,
  утечка check ID в prompt, граница C3 2→3, существующий output; placeholder
  (`""`, `{}`, `False`, `0`) очищает pending, audit не пересчитывает inputs.
- Task 2 (copies): сортировка по `Path.parts`, audit без mode/`.git`/peers, symlink,
  alternates, hard-link, remote, inventory hash, preserve без hash, restore без chmod,
  captured path check, нет exclusions, archive HEAD вместо base, AGENTS не очищен,
  `create_home exist_ok`, non-empty memory, leftover target/.letopis, Superpowers
  bullet; Docker: T2 probe монтирует oracle → тест падает. Две первые мутации
  (alternates, captured path) пережили тест из-за маскирующей проверки; тесты
  усилены, затем мутации убиты.
- Task 3 (oracles): неверная версия/help, неповреждённый bundle, нет collision,
  swapped vectors, duplicate attempt, порог 2→3, bypass audit, skip
  `require_unchanged`, infra как false; fix round: vacuous config grade, text без
  нового vector, timeout как rejection, inherited env, потерянные verdicts,
  build cause, reindex/parse как infra, raw probe value.
- Task 4 (environment/telemetry): auth read, слияние uncorrelated counters,
  cached прибавлен повторно, ready при missing config, без always-pending, дубликат
  дважды, conflict first-wins, потеря unmatched, unknown→0, retry overwrite, stale
  image, loose version, network, полный env в docker, invented session ID, demo
  `pilot`, `.git`/auth в context, corrupt asset, egress, memory script drains,
  intValue строкой, context unchecked, partial order, missing assets, absent image,
  preflight exit 0, receipts не слиты; fix round: no events/missing collector,
  git/target и participant pending, hardcoded `request.id`. Мутация F1a (`bool(rows)`)
  пережила тест — guard признан избыточным и удалён.

## Критерии и evidence

Spec §9 и карточка: каждый критерий — evidence или PENDING с владельцем.

| Критерий | Evidence сейчас | Остаётся (владелец → следующий шаг) |
| --- | --- | --- |
| Девять реальных задач, frozen prompts, check IDs, reuse_from, RED на seed | `selection.json` (3×3, #53/#43/#49), `test_study.py`; seed RED выше | **PENDING** `prompt_freeze` (человек читает 9 prompts и `instructions.md`; черновые места: C3-T1 coordinator sentence с `wire_embedder`/`Embedder::drain`, ссылки C1-T3/C2-T3 на X-1); **PENDING** `base_drift_recheck` на финальной base (оператор): не решены ли задачи там уже |
| Парные старты идентичны, изоляция цепочек/условий, пустая память, новая сессия на слот, нет write-back | `test_copies.py` (tamper и write-back matrices), audit без `seed_drift`, Docker boundary proof PASS, 18 templates без session ID | **PENDING** `participant_isolation`, `preflight:git_target_separation` (TK-31 → команда запуска участника с тем же mount builder); seeds исключают `docs/zkit/letopis/checks/TK-30`…`TK-33` (там check IDs, причины seed RED, правила oracle и `bounded_buffer` audit); общие карточки TK-5/TK-17/TK-30…33 и `checks/TK-5` намеренно остаются (одинаковы в обоих условиях, без check IDs/oracles — Ruling R10) |
| Тот же agent/model/config | Поля manifest `model`, `agent_config` есть и null | **PENDING** model, revision (или явно `unknown`), бюджет time/turns/tokens/retries, SHA-256 общей agent config (оператор) |
| Локальная инфраструктура и негативные controls без модели; demo отдельно | 150 focused tests на `8aba7d0`, P-1 31 tests; demo: `test_records.test_demo_passes_p1_checker_as_demonstration_only` (Task 4, входит в focused suite) запускает P-1 checker → `demonstration_only`, `agent_executions=0` | — |
| Freeze product/model/config/telemetry и gate prerequisites; unready | `audit` exit 2 (в т.ч. evidence-gates `preflight`, `participant_isolation`, `correlation_attribute` — без них unready даже при всех прочих значениях), `preflight` всегда exit 2 | **PENDING** `preflight`, `participant_isolation`, `correlation_attribute` (evidence-объекты, интерпретирует человек); `product` (финальная post-TK-29 base, после TK-29); `memory_versions` (версии memory binary/skill, после TK-29); `embedding` (provider/model/config полного профиля, оператор); `assets` (pinned `codex-linux.tar.gz`, `code-mode-host.tar.gz` и digest финального image с `letopis`, оператор); `g2` — сопоставляется с evidence TK-29, **evidence пока нет** (оператор после TK-29) |
| Real model access и полный usage | — | **BLOCKED без разрешения**: `model_access`, `embedding_access` (отдельно разрешённый paid запуск TK-31); атрибут корреляции `request.id` не подтверждён — сверить с настоящим collector capture Codex 0.164.0 до TK-31 |
| Источники, revisions/hashes, границы; нет платных сессий/решений/merge/push | Этот отчёт, receipt, пересчитанные TK-17 hashes | — |
| Архитектура (карточка) | Раздел ниже | — |

## Архитектура и границы

Проверенные файлы — только operator tooling и docs, вне продукта:
`tests/final_pilot/{study,run,copies,evaluation,environment,telemetry,records}.py`,
`tests/final_pilot/oracles/{cli,delivery,drain}.py`, `tests/final_pilot/oracles/service_probe.rs`,
`tests/final_pilot/test_{study,copies,oracles,environment,records}.py`,
`tests/final_pilot/README.md`, `docs/zkit/letopis/pilot/final/{selection.json,instructions.md,readiness.md}`,
`docs/zkit/letopis/checks/TK-30/`.

`git diff --stat d260348..HEAD` содержит только эти пути плюс spec/plan и карточку
`docs/zkit/letopis/roadmap/epics/EP-5-final-pilot/tasks/TK-30-pilot-setup.md`; нет
изменений `src/`, `Cargo.*`, schemas, `skills/letopis/SKILL.md`, `tests/pilot_protocol`
и P-1 protocol. Product Rust не менялся, поэтому новых adapters, services, wiring в
`src/lib.rs` или зависимостей нет; общий порядок проверки структуры к продукту не
применяется. `service_probe.rs` — private oracle source оператора: копируется в
disposable evaluation copy как `examples/tk30_service_probe.rs` после `make check` и
собирается там offline; в participant inventory его нет (seeds исключают
`tests/final_pilot`, `tests/pilot_protocol`, `docs/zkit/letopis/pilot`, `docs/superpowers` и
`docs/zkit/letopis/checks/TK-30`…`TK-33`; тест `test_seed_has_no_frozen_check_or_agreement_id`
и поиск всех frozen check/agreement IDs и `tk30_service_probe` по шести реальным seeds —
0 совпадений). Seed product
paths (`src`, `Cargo.toml`, `Cargo.lock`, `skills/letopis/SKILL.md`, `tests/support`)
равны base по Git tree.

## Пределы и допущения

- C3 oracle кодирует drain semantics seed: при изменении текста во время drain новый
  hash получает vector в том же drain; при изменении config под новой identity vector
  не появляется. Изменение семантик в финальной base требует пересмотра до freeze.
- C3 не имеет end-to-end GREEN control (конкурентной реализации нет).
- `infra_error` с `execution.infra_cause` (`participant_build`/`probe_compile`/
  `environment`): несобирающийся патч участника не равен failed feature; TK-33
  обязан читать это поле.
- `bounded_buffer` требует operator annotation с refs на captured commit; без неё
  `null`. Annotation не писалась.
- `audit` корректен только для свежего root; capture/require_unchanged хэшируют все
  шесть conditions (слоты последовательные). Повторный `evaluate` прежнего слота после
  запуска любого следующего слота падает с `participant state changed`.
- `evaluate` исполняет `build.rs`/тесты участника как оператор: запускать без credentials
  (отдельный пользователь или контейнер без сети/`~/.codex`) и с root вне checkout.
  Хэши operator sources, `inputs/` и capture/blobs до/после оценки **обнаруживают**
  подмену (exit 2), но не предотвращают её.
- Release lock в C3-T3 засчитывается только по наблюдённому `status` (exit 0, `ok:true`
  объект); сбой/мусор `status` — не «released».
- `container_isolation` и Docker `test ! -e` canaries доказывают только собственные
  mounts preflight/теста. Memory probe не проверен на image с `letopis`.
- Хост `codex --version` однажды выполнялся вне preflight при разработке Task 4
  (auth не читался, модель не вызывалась) — отклонение от процедуры, не evidence.
- Отложенные minor findings не исправлялись: например `drain.py` 616 строк,
  `copies.py` 531, `test_copies.py` 871; malformed annotation поднимает исключение;
  `audit` падает traceback на non-dict manifest. Python gate `run.py` поднят до 3.11.4
  (tarfile `filter="data"`).

## Передача

Готово: prospective selection и черновые prompts, парные seeds, 9 private oracles с
seed RED, credential-free preflight/telemetry/templates, focused tests. До TK-31
требуются: финальная post-TK-29 base и повторные `prepare`/`audit`/seed RED на ней,
G-2/TK-29 evidence, prompt freeze, pinned assets и digest image с `letopis`,
model/revision/budget/config, embedding config/профиль, версии memory binary/skill,
подтверждение атрибута корреляции на реальном capture и отдельное разрешение
на платные запуски TK-31/TK-32. Merge, push и закрытие issues не выполнялись.

## История ревизий

Короткие SHA в этом отчёте и в receipt (`4c308cd`, `74e0f82`, `8aba7d0`, `head_*`) — ревизии истории ветки до её
squash в логические commits; самих этих commits больше нет. Соответствие: `4c308cd` → дерево `d4b2386`
(идентично); `8aba7d0` → `7331187` (operator code идентичен, docs отличаются); `74e0f82` — промежуточное
дерево, его изменение кода входит в `7331187`. Operator source отчёта: `73311873254c3399a2fc133d9d45064a2807c7c6`.
