# TK-30: операторская подготовка финального пилота

Model-free инструменты оператора для [TK-30](../../docs/zkit/letopis/roadmap/epics/EP-5-final-pilot/tasks/TK-30-pilot-setup.md):
проверка prospective selection, парные независимые seeds, одноразовая private оценка
девяти задач, credential-free проверка окружения и нормализация supplied telemetry.
Инструменты **не** читают credentials, не запускают `codex exec`/агента, не вызывают
модель или provider и ничего не скачивают. Подкоманды `session`/`exec` намеренно нет.
Текущее состояние подготовки и блокеры — в [отчёте TK-30](../../docs/zkit/letopis/checks/TK-30/report.md).

Python **3.11.4+** (`tarfile` extraction с `filter="data"`), только standard library.
Системный `python3` на macOS-хосте разработки — 3.9.6, поэтому все команды ниже
используют `python3.12`; `run.py` на Python < 3.11.4 завершается с JSON ошибкой и кодом 2.

## Команды

```sh
python3.12 -B -m unittest discover -s tests/final_pilot -p 'test_*.py'

python3.12 -B tests/final_pilot/run.py prepare --repo "$PWD" \
  --base FULL_40_HEX_OID --output /OUTSIDE/THE/CHECKOUT/preparation-NEW
python3.12 -B tests/final_pilot/run.py audit --root /OUTSIDE/THE/CHECKOUT/preparation-NEW
python3.12 -B tests/final_pilot/run.py evaluate --root /OUTSIDE/THE/CHECKOUT/preparation-NEW \
  --slot C1-T1-baseline
python3.12 -B tests/final_pilot/run.py context --source DIR --assets DIR --output NEW_DIR
python3.12 -B tests/final_pilot/run.py preflight --root DIR --image LOCAL_IMAGE
```

Пути `--root`/`--output`/`--repo`/`--source`/`--assets` можно передавать
относительными: `run.py` разрешает их от текущего каталога оператора до запуска
команды, а `evaluation.evaluate` дополнительно разрешает root, поэтому cargo и
поиск бинарника используют один абсолютный `CARGO_TARGET_DIR` (дефект Task 5
исправлен, тест на относительный root в `test_oracles.py`).

**`evaluate` исполняет код участника как оператор** (`build.rs`, тесты `make check`):
запускать его только на хосте/пользователе **без credentials** (отдельный пользователь или
контейнер без сети и без `~/.codex`) и с preparation root **вне checkout**. До и после
каждой оценки `evaluate` хэширует operator sources (`evaluation.py`, `oracles/`, probe),
`inputs/` и capture/blobs слота и при расхождении завершается с ошибкой (exit 2). Это
**обнаружение** подмены, не её предотвращение (containment нет).

Каждая команда печатает один JSON-объект. Exit 0 — локальная операция завершена
(для `evaluate` — также при failed/null checks и `infra_error`); exit 2 — неверные
inputs/инфраструктура **или** unready. `audit`, `context` и `preflight` возвращают 2,
пока есть pending: `preflight` всегда добавляет `preflight:git_target_separation`,
`participant_isolation`, `model_access`, `embedding_access`, поэтому локальный smoke
никогда не даёт exit 0. `audit` сам требует evidence-gates `preflight`,
`participant_isolation` и `correlation_attribute` (объект с непустой ссылкой `evidence`),
поэтому без них остаётся unready, даже если заполнено всё остальное; интерпретация
evidence — ручной шаг. Output/evidence directories всегда новые; `.artifacts/`
игнорируется Git, но preparation root для `evaluate` должен лежать вне checkout. Private evidence (seeds, captures, oracle outputs, логи) не коммитится.

## Opt-in Docker proofs

Без переменных окружения оба теста пропускаются (`skipped=2`), и пропуск надо называть
пропуском. Используются только уже локальные images; ничего не скачивается и не собирается.

- `TK30_CONTAINER_OUTPUT=NEW_DIR` (и необязательно `TK30_TEST_IMAGE`, по умолчанию
  `letopis-tk17-codex:v2-0.164.0`): `ContainerBoundaryTests` в `test_copies.py` —
  toy `build.rs` читает private marker и пишет только в evaluation copy; будущий
  T2-контейнер не видит oracle/target/marker; unsafe shared mount обнаруживается;
  контейнер с timeout удаляется по собственному имени.
- `TK30_PREFLIGHT_IMAGE=LOCAL_IMAGE`: `LocalImagePreflightTests` в
  `test_environment.py` — настоящий `preflight` против локального image
  (`--pull never --network none --cap-drop ALL`, без mounts/env).

## Порядок и стоимость

- `audit` проверяет **только свежий** preparation root: `audit_seeds`/`verify_layout`
  описывают нетронутые seeds. После запуска слотов (появились `target`, `homes`,
  memory) `audit` сообщит `seed_drift` — это не дефект; для слотов существуют
  `capture_slot`/`require_unchanged`.
- `capture_slot` и `require_unchanged` хэшируют полный inventory **всех шести**
  conditions, включая `.git`, `target` и HOME слотов. На реальных seeds это
  гигабайты после сборок; слоты выполняются последовательно, параллельных scored
  слотов нет. Время `evaluate` одного слота на хосте ≈ 215–250 с, в основном `make check`.
- Повторная оценка того же слота создаёт новый `operator/evidence/SLOT/run-N`; прежние
  runs не перезаписываются, copy пересобирается из captured bytes. Ограничение: оценка
  сверяет участника с audit всех шести conditions, снятым при capture слота, поэтому
  после запуска **любого** следующего слота (любой цепочки) повторный `evaluate` прежнего
  слота падает с `participant state changed`; переоценка возможна только до следующего слота.

## Семантика результатов

- `status` — `assessed` или `infra_error`; check — `{id, passed: true|false|null, evidence}`.
- `execution.infra_cause`: `null` для assessed; `participant_build` — captured source
  не собрался (cargo build); `probe_compile` — private service probe не компилируется
  на этом source; `environment` — сбой хоста (tool не стартует, rustc/git, loopback).
  При `infra_error` вычисленные раньше verdicts сохраняются (например
  `regression=false`), остальные — `null`. Downstream (TK-33) обязан читать
  `infra_cause`: участник с несобирающимся патчем помечен infra, а не failed feature.
  Сбой `reindex`/parse участника внутри C3 fixtures — product failure (`false`), не infra.
  `make check`, остановленный watchdog, даёт `regression=null`.
- `bounded_buffer` (C3-T1) — комбинированный критерий: barrier-наблюдение двух
  удерживаемых HTTP requests **и** ручной operator audit captured allocation/queue
  paths. Annotation `operator/annotations/SLOT.json` обязана содержать
  `captured_commit` = HEAD evaluation copy, bool verdict, refs `{path,line,symbol}`
  (symbol присутствует на этой captured строке) и вывод. Без неё — `null`, не PASS.
  Malformed annotation сейчас поднимает исключение, а не даёт `null` с пометкой.
- Допущения C3 oracle о drain semantics seed: при изменении текста заметки во время
  drain новый hash получает vector **в том же drain**, старый hash остаётся без vector;
  при изменении config во время drain под новой identity vector **не** появляется.
  Если финальная post-TK-29 base меняет эти семантики, oracle надо пересмотреть до freeze.
- C3 не имеет end-to-end GREEN control: конкурентной реализации drain нет. GREEN
  покрыт literal-observation grader tests и real-socket overlap/sequential observers.
- Telemetry pairing: `normalize_events` спаривает request и `response.completed`
  только по атрибуту, явно названному в `context["correlation_attribute"]`. Имя
  `request.id` в тестах — **неподтверждённое допущение**; его нужно сверить с
  настоящим collector capture Codex 0.164.0 до TK-31. Без атрибута или без события
  usage остаётся unknown, `complete=false`, не ноль.
- `records.build_templates`/`write_demo` не имеют подкоманды `run.py`; demo — только
  P-1 `kind=demonstration`, не наблюдения пилота.

## Известные ограничения

- `container_isolation` в preflight доказывает изоляцию лишь собственного
  preflight-контейнера; изоляция участника и разделение Git/target — pending до
  команды запуска участника (TK-31).
- Docker `test ! -e` проверки host-путей в boundary proof доказывают только mounts
  самого теста; будущая команда участника обязана использовать тот же mount builder.
- Memory probe не проверен на финальном image: локальный TK-17 image содержит
  `letopis-slice`, а не `letopis`, поэтому preflight `tools`/`memory` — false.
- `Dockerfile` recipe (из TK-17) при сборке скачивает (`apt-get`, `cargo fetch`);
  digest закрепляется после сборки.
- Seeds содержат карточки TK-5/TK-17/TK-30…TK-33 и `checks/TK-5` (одинаково в обоих
  условиях); решение об их исключении — за человеком до freeze.
