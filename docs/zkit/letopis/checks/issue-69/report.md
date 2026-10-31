# Issue #69 — явный path в marked Markdown links

Дата: 2026-10-31. Состояние: реализация Tasks 1–4 подготовлена; независимый review
Task 4 и whole-branch review выполняет controller после этого handoff. Этот отчёт
не утверждает, что review, интеграция или закрытие issue уже выполнены.

Проверенная продуктовая ревизия: `1d910911fecc18e06aa07a591816d9f915525c5b`,
полный tree `2f8c5badf7c901f677e7498ee5ea2fdab9b1287d`. Task 4 меняет только
активные документы/skill/корпус и этот отчёт; src/tests/build/vendor не изменены.
[Спецификация](../../../../superpowers/specs/2026-10-31-issue-69-markdown-link-path-design.md),
[план](../../../../superpowers/plans/2026-10-31-issue-69-markdown-link-path.md),
[X-4](../../contracts/X-4-markdown.md), [D-106](../../decisions.md#D-106).

## Результат и границы

Единственный title — `letopis:type=TYPE;path=ADDRESS`; terminal `path` задаёт
canonical target без дополнительного decode/trim. Локальный destination
проверяется лексически относительно документа, с одним percent decode UTF-8,
без fragments/existence I/O. Remote HTTP/HTTPS sources исключены из сравнения.
`LinkPathMismatch` публикуется через существующее files.error; check/status
показывают безопасный reason link_path_mismatch. Status read-only и отражает
только последнюю публикацию, включая очистку ошибки. Явный reindex после upgrade
обязателен, затем check; legacy fallback/автоматического rewrite нет.

Обновлены X-4, сводка контрактов, MC-1/диагностическая семантика, отдельное датированное
решение D-106, development upgrade instructions и Letopis skill/operations.
Старая TK-20 процедура в development явно помечена исторической. Исторические
specs, plans прошлых задач и raw receipts не переписаны. Все 10 действующих
`letopis/architecture/*.md` используют canonical path и document-relative href.
Сохранены created_at и все canonical targets, кроме честно исправленного
удалённого `src/git/cli.rs` → `src/git/repository.rs`.

`git-snapshots.md` теперь описывает прочитанную текущую GitRepository/gix реализацию:
открытие work tree, snapshot operations, отказ alternates и повторный guard на
входе операций. Новый basis — указанный полный HEAD tree; `git show
<tree>:src/git/repository.rs` побайтово совпал с прочитанными 7847 bytes, SHA-256
`0230dfa7ead8a4521e2587416912a30460f7ba8537fc59903451db1024af1baa`.
Это не слепая замена OID; временный авторский index не понадобился. Реальные
HEAD/index не менялись ради provenance. Receipts: `.artifacts/issue-69/task-4/git-snapshots-basis.json`.

## Проверки Task 4

Все новые локальные receipts сохранены в `.artifacts/issue-69/task-4/`.
Успешный командный результат не превращён в сумму nested `test result` строк:
root suite запускает собственные child fixtures, поэтому уникальный общий счёт
root tests здесь не выводится.

| Команда / проверка | Результат и evidence |
| --- | --- |
| `make verify` на host macOS ARM64 | PASS, exit 0, вывод `ok`; `make-verify.log`: fmt, all-target check, Clippy `-D warnings`, debug root tests, strict Rustdoc, release root tests. |
| `python3 .artifacts/issue-69/task-4/check_docs.py` | PASS; created_at/targets/bases, relative hrefs, новые локальные links/anchors, отсутствие active legacy declarations; `doc-check.json`. |
| `git diff --check` | PASS на итоговых owned paths. |
| `python3 .artifacts/issue-69/task-4/run_corpus.py` и `check_corpus_freshness.py` | PASS; реальный `target/debug/letopis`, отдельный `/private/tmp/issue69-task4-corpus-lkvuppf2`, скопированные source objects без alternates, init → disabled config → reindex → check → status → inspect → scoped searches. `corpus-receipt.json`, exact command responses `corpus-*.json`. |
| Offline Linux ARM64 parser/lifecycle | PASS; exact current input hashes, команды ниже; `linux-inputs.json`, `linux-receipt.json`, `linux-1.log`, `linux-2.log`. |

Изолированный active corpus: 10/10 files valid, 11 knowledge elements, 26 edges;
check errors `[]`, status problems `[]`, revision 1, embedding backlog/failed 0.
Inspect вернул один retained tree; байты repaired knowledge и repository.rs
сверены с этим tree. Embeddings явно disabled, provider/model null, parameters
пусты; никакой реальный provider/model не запускался. Пользовательские
`.letopis/config.json`, cache, vectors, locks и Git refs команды не затрагивали.

Однако это **не** полная перепроверка всех knowledge claims. Отдельное точное
сравнение старых basis bytes с текущими исходниками выявило шесть изменённых
bindings у четырёх знаний; search с `filters.needs_recheck:true` возвращает у них
`source_changed`:

| Знание | Изменённый источник |
| --- | --- |
| command-wiring.md | src/commands.rs |
| component-boundaries.md | docs/zkit/letopis/architecture.md, src/lib.rs, src/service/mod.rs |
| embedding-execution.md | src/service/search.rs |
| search-pipeline.md | src/service/search.rs |

Их basis `1fba9cfb01cddb4e763b1d33580f48000952ea74` сохранён; claims не были
переподтверждены заменой OID. Чистые check/status не означают отсутствия source
freshness flags. Полные SHA-256 обеих версий каждого binding и реальные search
responses сохранены в corpus receipt.

Linux использует существующий image
`letopis-issue67-final-fix-20261031-v3:latest`, id
`sha256:05ad58ffe40619db2e8231a145571fc0f843d3962072caa26da80ba43f0f07d7`,
server Linux/aarch64 glibc 2.36; Rust/cargo 1.95.0, Git oracle 2.50.1.
Network выключен, image pull отсутствует. Image src/tests/vendor удалены в
**disposable container**, затем скопированы точные текущие входы из read-only
checkout; caches/toolchain образа сохранены. 450 host/container input SHA-256
совпали. Первоначальный bind-mount запуск завершился до старта тестов: remote
Docker daemon не видит `/private/tmp` host. Повтор использовал tar/docker cp,
затем контейнер удалён. Это проверка текущего кода, а не старых image tests.

```sh
cargo test --locked --offline --test markdown_link_paths --test markdown_links --test markdown_elements --test markdown_search --test markdown_link_lifecycle
cargo test --locked --offline --lib markdown::links::tests
```

Обе команды exit 0; пять integration targets прошли (11/6/10/7/9 tests согласно
их собственным summaries), три private decoder tests прошли.

## Повторно используемые receipts Tasks 1–3

Task 4 не меняет Python tooling: SHA-256 всех 29 Python входов corpus/performance/
TK-24 побайтово совпадают с финальным Task 2 commit
`c7414d60dd07d73edadc1857cfb21817343d71ee`; `python-input-parity.json`.
Поэтому без повторения неизменённых suites использованы:

- `PATH=/opt/homebrew/bin:$PATH python3 -B -m unittest discover -s tests/target_corpus -p 'test_*.py'` — PASS, 50 tests, 1 optional skip; Task 2 `target-corpus.log`.
- `PATH=/opt/homebrew/bin:$PATH /opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py'` — PASS, 81 tests, 8 optional skips; Task 2 `performance-committed-final-retry.log`.
- `/private/tmp/letopis-issue-61-schema-venv/bin/python -B tests/cli/tk24_workflow.py --binary "$PWD/target/debug/letopis" --evidence /private/tmp/issue69-tk24-task2-20261031-1` — PASS; Task 2 `tk24-workflow.log`.

Эти Python receipts принадлежат Task 2 revision, не объявлены новым Task 4
запуском на более позднем binary. Ранее неизменённый performance
`test_failed_required_counter_marks_profile_campaign_incomplete` один раз упал
с StopIteration/отсутствующей measured series, затем прошёл отдельно и в
единственном финальном complete retry. Глубинная причина не установлена;
этот финальный PASS её не доказывает. Восстановленные readiness/cancellation
тесты Task 2 проверяют реальное измерение и SIGALRM после readiness без изменения
runtime budgets; initial failures сохранены в прежних receipts.

Чувствительность новых behavioral tests подтверждена в Tasks 1–3 временными
компилируемыми mutations и полным byte restoration; Task 4 docs-only новых
продуктовых mutations не требует. Примеры exact commands/результатов:

| Временное нарушение | Команда после `cargo test --locked --offline` / Python | Outcome |
| --- | --- | --- |
| убрать local mismatch rejection | `--test markdown_link_paths rejects_mismatched_and_unsupported_navigation_paths` | assertion FAIL, потом restored PASS; Task 1 `mutation-comparison.log`. |
| брать graph target из destination | `--test markdown_link_paths uses_metadata_ranges_even_when_navigation_is_a_canonical_address` | literal target FAIL; Task 1 `mutation-destination.log`. |
| percent-decode title | `--test markdown_link_paths decodes_only_navigation_paths_once` | FAIL; Task 1 `mutation-metadata-decoding.log`. |
| разрешить controls/backslash | `--lib rejects_literal_and_encoded_controls_and_backslashes` | typed error FAIL; Task 1 `mutation-controls.log`. |
| сломать cached classification | `--lib classifies_every_known_cached_message_without_changing_parser_diagnostics` | safe reason/message FAIL; Task 1 `mutation-classifier.log`. |
| bypass percent parsing / lossy UTF-8 | `--lib rejects_malformed_navigation_percent_escapes` / `--lib rejects_malformed_navigation_utf8` | оба compiled assertion FAIL, затем restored PASS; Task 1 `review-fix-1/`. |
| убрать independent navigation oracle / вернуть старую external mask | Task 2 focused Python tests, точные argv в его локальном отчёте | оба FAIL, restored PASS; `mutation-navigation.log`, `mutation-controls.log`. |
| убрать durable mismatch file row / не reparsing unchanged при reindex | `python3 .artifacts/issue-69/task-3/run_mutations.py` | три compiled assertion FAIL (включая classification), три restored PASS; Task 3 `final-mutations/mutations.json`. |

## Review и ограничения handoff

Self-review Task 4 проверил все physical files, текущую реализацию GitRepository,
canonical identities, source bases, hrefs, created_at, upgrade policy, diagnostic
timing, active/historical scope, отсутствие production/visibility/trait/import/
lib.rs wiring changes. Совпадение build/test inputs исключает оставленные
продуктовые mutations. Независимые Task 4 review и whole-branch review остаются
controller-owned и здесь не помечены завершёнными; их выводы/исправления должны
быть добавлены после проверки.

Не запускались x86_64, полный Linux root gate, storage/native platform runner,
исторические harnesses с отдельным manifest, power-loss, реальные providers,
viewer compatibility, resource/performance campaign или полный pilot acceptance.
Storage/native inputs не менялись; root tests и focused Linux tests не означают
новую независимую проверку standalone linkage/native capabilities. Нет merge,
push, публикации issue или закрытия issue #69.
