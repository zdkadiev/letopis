# TK-14 — Сквозной CLI и начальный навык агента

> Статус: done; fast-forward в локальный main, GitHub issue #14 закрыта. Push не выполнялся.
> Дата: 2026-10-24. Production base: `ae7d2ebf377af9efa46d53ab157cde7d8526be87`.
> Frozen code/skill checkpoint: `b7d3b44`; Native, новая ветка `codex/TK-14`.

## Результат и границы

TK-14 соединяет существующие Markdown/Git/SQLite/search/inspect/embedding компоненты
пользовательским сценарием. Новых product services, runtime, SQL schema или Cargo
зависимостей нет; `src/`, `Cargo.toml`, `Cargo.lock`, `build.rs` и vendor не менялись.
Новые проверки работают настоящим CLI и реальным provider transport, с controlled
loopback Ollama HTTP, а не с реальной моделью. SQL/vector seed не используется.

Начальный переносимый навык: [skills/letopis/SKILL.md](../../../../../skills/letopis/SKILL.md).
README содержит реально выполненный loader: source commit/tree → Markdown templates →
product CLI refresh. Для TK-15 меняется набор входов, а не путь публикации. Генератор
100k/100k/500k, RAM и usefulness pilot остаются TK-15…17.

## Приёмка

| Критерий карточки | Проверка и текущий результат |
|---|---|
| Новая сессия сохраняет знание/ссылку; CLI находит/inspect; current vector | 6 actual-CLI scenarios PASS; отдельные fresh writer/reader PASS; current vector bytes проверены Rust-тестами, key присутствует в trial |
| Сбой не смешивает файлы/БД | CLI invalid-neighbor/provider/text-race PASS; свежие service-process SIGKILL/rollback PASS |
| Начальный навык объясняет путь | Реальный baseline RED без навыка; writer/reader GREEN с тем же навыком |
| Source projection/загрузка для RAM fixture | 3 knowledge, 2 source, 5 рёбер; независимые ID/timestamps/hash literals; повторная загрузка идемпотентна |
| Структура и границы | Production delta отсутствует; filesystem/Git/storage/HTTP остаются за прежними application traits; независимое review PASS |

## Проверяемые сценарии

`tests/thin_slice.rs` содержит шесть тестов:

- `binary_load_projects_sources_and_is_idempotent`: actual CLI, 3+2 nodes/5 edges,
  root-relative whole/range source IDs, minimum created_at, literal byte hashes и
  search_hash, отсутствие лишней revision при повторе, сохранность user Git/corpus.
- `binary_inspect_retains_knowledge_bytes_separately_from_source_basis`: один knowledge
  tree, точные Git bytes и metadata; последующая правка live file не меняет tree.
- `binary_search_returns_before_background_embedding_and_reuses_vectors`: foreground
  завершается до ответа HTTP; detached drain хранит актуальный vector по literal hash,
  три document requests и ноль повторных после drain.
- `binary_provider_failure_preserves_fts_and_bounds_attempts`: 503, attempts=1 на текст,
  безопасная диагностика, FTS/корпус сохранены, исчерпанная работа не перезапускается.
- `binary_invalid_file_keeps_last_projection_and_publishes_valid_neighbor`: один bad
  Markdown не блокирует валидную правку; current inspect возвращает validation_failed;
  исправление файла снимает ошибку через следующий CLI refresh.
- `binary_text_edit_during_document_http_requires_its_own_vector`: публикация нового
  текста во время старого HTTP, отдельный запрос нового текста, current new hash/vector,
  удаление orphan old vector.

`THIN_SLICE_RESPONSE` receipts проверяются по X-1 отдельно для каждой команды;
`capture_thin_slice_responses` требует все семь cases, правильные exit/error codes и
валидные envelopes. Runner tests до collector дали реальный RED (missing function),
после реализации — 20/20 GREEN; добавление skill inventory также RED→GREEN.

Новые characterization tests на существующем исправном продукте сразу прошли.
Первый missing-helper compile error не считается behavioral RED. Чувствительность
подтверждена отдельными компилируемыми controls ниже.

## Compiling controls

Каждый control выполнен в disposable source copy: compile exit 0 → один named test
runtime exit 101 → exact byte restore → тот же test exit 0. Исходный worktree не
мутировался; cargo-mutants не запускался.

| Control | Поломка | Observer |
|---|---|---|
| source-timestamp | source created_at + 1 | literal minimum timestamp |
| inspect-blob | добавлен LF в captured Git blob, metadata прежняя | независимое сравнение Git bytes |
| vector-contents | значения vector инвертированы | exact persisted vector |
| attempt-budget | max_attempts + 1 | attempts=1 и HTTP count |
| empty-fts | публикация пустого search text | actual CLI membership |
| orphan-vector | отключена очистка unreferenced vectors | old vector отсутствует после drain |

Raw outcomes: `.artifacts/tk14/controls-task1-01/outcomes.json` (2 controls) и
`controls-task2-01/outcomes.json` (4 controls), рядом compile/RED/restore-GREEN logs.
Task 1 controls получены до переноса fixture под уже существующий Markdown subtree
и локального подключения helper; import/path delta не меняет тела тестов или продукта.
Task 2 controls используют те же Rust-тесты, что final checkpoint. Исторические
receipts не переименованы в доказательства другого source revision.

## Отказ процесса и границы доказательства

На текущем source отдельно выполнены:

```sh
cargo test --locked --offline --lib service::document_refresher::tests::process::sigkill_keeps_exact_old_or_complete_new_publication -- --exact --nocapture
cargo test --locked --offline --lib service::document_refresher::tests::process::late_storage_error_rolls_back_every_cache_mutation -- --exact --nocapture
```

Оба PASS: первая проверяет десять подтверждённых границ SIGKILL и последующий refresh,
вторая — rollback поздней ошибки. Это private process driver реального document
service; не actual-CLI injection и не power-loss test. CLI wiring проверяется отдельно
новыми binary scenarios. Дополнительных product test hooks не введено.

## Проверка навыка свежими агентами

- Без навыка `/root/tk14_skill_baseline` (4 tools) создал обычный Markdown без Letopis
  metadata; `--help`, no args и `help` вернули invalid_request. Controller независимо
  вызвал actual search: пустой результат. Это behavioral RED, а не оценка качества модели.
- С навыком `/root/tk14_skill_writer` (6 tools) создал знание с created_at, проверенным
  source tree и external-ссылкой на `src/cache.rs#L1-L3`; actual search/inspect/Git read
  успешны. Первый sandboxed CLI отказал в доступе к embedding service; разрешённый
  escalated retry успешен. Это ограничение тестового окружения, не исправленный product bug.
- `/root/tk14_skill_reader` (5 tools), без writer transcript/ответа, нашёл решение
  через CLI, прочёл knowledge snapshot и source basis, восстановил наблюдение:
  `use_cache()` безусловно возвращает true. Эта простая проверка не является pilot.
- Controller проверил точные knowledge bytes, source metadata/tree/edge, schema-ready
  CLI envelopes и current vector key. Для этого trial использовалось read-only SQLite
  наблюдение metadata/key presence, без SQL writes/seed; точные vector values отдельно
  проверяются public Storage read API в Rust scenarios.

Raw evidence: `.artifacts/tk14/agent-skill-01/` — setup, baseline-verdict,
writer-verdict, final-verdict, HTTP records и disposable repositories. Binary SHA-256
`0377363ce21d78de11298eb71b9703dfad5ab69960ae5001e1f8faf75540a534`.
Skill SHA-256 `153e0f7c1b6f6c5397198a7163d64f78de551a510f29615f72b0636c6e55733a`.
Общий HTTP fixture завершён после trials. Глобальная установка skill и auto-discovery
во всех runtimes не проверялись.

## Platform gates

Host-01 PASS на frozen `b7d3b44217c4e8e95c31e3d0b6f2c9311ce97d2b`: root **460 passed / 0 failed / 4 ignored** в каждом debug/release профиле; 16 nested filtered helper summaries за оба профиля посчитаны отдельно. Fmt/all-target check/Clippy/strict Rustdoc и actual CLI/drain/thin-slice receipts PASS. Host manifest содержит 152 input hashes; root log SHA-256 `8cba3323fe4753a8cacad4c6398337e37e4e0a8cd681a78f23e83fe78bd6150e`.

Linux-01 FAIL в неизменённом тесте `git::cli::tests::hardens_tree_and_ref_writes_with_git_fsync_settings`, до argv assertions: Git spawn `ExecutableFileBusy` (`src/git/cli/tests.rs:426`), root debug 180 passed / 1 failed / 3 ignored. Новые интеграционные targets и release ещё не выполнялись. Та же ошибка уже сохранена в TK-12, и ни тест, ни `TempRepo::write_script` здесь не менялись.

В том же image `sha256:30157815f9f8d7f8065ba99b51cf3c6b9e26a08e3c0cb38ad1b40e87df750955` точный тест прошёл **20/20** focused repetitions в owned network-disabled контейнере. Raw failure сохранён, `linux-repro-01` содержит команды/выходы. Причина за пределами наблюдаемого spawn error не доказана; workaround не внесён. Один полный unchanged-source retry в `linux-02` завершился PASS: root 460/0/4 в каждом debug/release профиле, 16 nested filtered helper summaries отдельно; семь thin-slice cases прошли X-1 validation. Первоначальный FAIL остаётся FAIL.

Python 3.12 / jsonschema==4.26.0 подготовлены в `.artifacts/tk14/python-env`; проверки dependencies не скачивают. Host, Linux outer и Linux inner manifests PASS; все 152 input hashes совпали между ними и с текущим деревом. Linux root log SHA-256 `ac169bb583b6b2d39cf4b84c741e03e477653f60e6e7f830ba29af85505407c8`. Область — macOS ARM64 и Linux glibc ARM64; fresh native standalone/storage harness отдельно не запускался: product storage/native/build/vendored inputs не менялись, добавлено только разрешение/copy skill в CLI Docker context, проверенное этим runner.

## Архитектурная проверка

Проверяются `src/lib.rs` wiring, `src/cli/handlers.rs`, application traits/service
implementations и отдельные Git/storage/embedding/filesystem/markdown/process
модули. Они не изменены в TK-14. Test helpers находятся под tests/support и
подключаются только к соответствующему target; source fixture не является product API.
Ordinary SQL не добавлен в product код. Независимый рецензент подтвердил эти границы и отсутствие Critical/Important findings.

## Итоговое независимое ревью

`/root/tk14_final_review`, gpt-6-astra/high, чистый контекст, весь branch diff
`ae7d2eb..b7d3b44`: **Ready to merge: Yes**. Critical/Important отсутствуют;
minor findings, требующих исправления или отсрочки, не перечислено. Рецензент
проверил все пять Review Focus, actual wiring, 152 checksum и содержательность
шести controls; полные suites повторно не запускал. Raw verdict сохранён в
`.artifacts/tk14/final-review.md`. После review меняются только completion docs,
не frozen code/skill/runner inputs.

Все рассмотренные, но отложенные области получили явное решение:

| Область | Решение и основание | Цена / предел доказательства |
|---|---|---|
| Постоянное устранение ExecutableFileBusy | Не заявляется: причина не доказана, unchanged retry подтверждает один snapshot | Нестабильность может повториться |
| Real model, RAM, масштаб 100k | Следующие TK-15…17; текущий fixture проверяет интеграцию | Resource fit и польза пока неизвестны |
| Полный source lifecycle/check/mixed inspect | Согласованная граница следующей фазы; skill сообщает ограничения | Эти операции полного v1 ещё недоступны |
| Physical power-loss recovery | SIGKILL/rollback имеют более узкую область; dedicated drill впереди | Устойчивость к реальному отключению питания не доказана |

## Решения исполнителя

1. Fixed inputs размещены в `tests/fixtures/markdown/thin_slice`, helper подключён
   локальным path module только к thin_slice target: существующие Docker COPY и
   recursive inventory уже покрывают их. Это избегает расширения остальных build
   harnesses. Цена при ошибке: source example находится в группе Markdown corpus
   fixtures вместо отдельного top-level каталога. Scope/bytes/ожидания не меняются.

2. После 20/20 focused PASS выполнен один полный Linux retry без изменения source,
   с сохранением первоначального FAIL. Это проверяет неизменный текущий snapshot,
   но не исправляет существующий sporadic fixture spawn failure. Цена при ошибке:
   он может повториться; успешный retry не доказывает постоянное устранение причины.

## Ограничения и передача

Нет утверждений о RAM/latency/100k corpus, полезности памяти, качестве реального
embedding, x86/Windows/NFS/power loss или полном source lifecycle. Check/mixed
source inspect/filters/traverse/status остаются соответствующим следующим задачам;
в навыке они явно помечены недоступными. G-1 не закрывается TK-14.

По команде пользователя 2026-10-24 выполнен fast-forward `main` с `ae7d2eb`
до `397d3d15475e3a2ff6eb538555aac70b78c03027`. На результате merge свежий
`make verify V=1` завершился с exit 0: 460 passed / 0 failed / 4 ignored отдельно
в debug и release; fmt/check/Clippy/strict Rustdoc PASS. Filtered subprocess
summaries не прибавляются к root counts. Лог: `.artifacts/tk14/integration-2026-10-24/main-verify.log`
в основном checkout. [GitHub issue #14](https://github.com/zaurkadiev/letopis/issues/14)
обновлена и закрыта как completed. Push не выполнялся; origin/main остаётся
на `ae7d2eb`. Worktree сохранён вместе с ignored raw evidence. Engram task: `entity_77b1f3b8be2c439397f96494020ee809`.

Execution ledger, task briefs и review package сохраняются отдельно в
`.artifacts/tk14/native-complete-b7d3b44/`; исходный plan workspace удаляется
только после побайтовой проверки архива. Raw gates/controls/trials остаются на месте.
