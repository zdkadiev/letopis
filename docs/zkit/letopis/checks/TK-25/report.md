# TK-25 — полный reindex и Git detection

**Состояние:** DONE — Task 1/2 реализованы, host-проверены, независимый whole-branch review принят без замечаний. Ветка готова к слиянию; merge/push/закрытие GitHub issue не выполнялись.

## Поведение

`letopis reindex` с `{}` полностью перечитывает и разбирает текущих Markdown-кандидатов под `index.lock`, использует общий per-file publish и одну cache write unit. Возвращает только `{revision}` внутри прежнего X-1 envelope; повторный вызов увеличивает publication counter. Начальное построение использует тот же полный путь, обычная правка остаётся incremental, status не пересобирает отсутствующий кэш.

Сохраняются last-valid nodes/edges/contentless FTS невалидного A и его авторское основание; валидный B публикуется, диагностика A и fingerprints фиксируются в той же unit. Фатальные capture/Git/provenance/SQL ошибки откатывают всю unit. Все source bindings повторно наблюдаются в полном режиме, включая last-valid declarations невалидных файлов. Векторы живут отдельно и переиспользуются по search_hash; reindex не вызывает inference и не ждёт drain/embed.lock.

## Проверки к этому checkpoint

- Чистая база `913b2b7`: `make check` PASS; quiet вывод не содержит точных счётчиков.
- Task 1 `fa87c97`: `make check V=1` PASS, **753 passed / 0 failed / 5 ignored**, 53 executable + Doc-tests. Частные refresher-проверки: 32 passed / 1 ignored child, который явно запускают process schedules. Реальные репозитории: 16 прежних refresh + 5 новых reindex tests PASS.
- CLI: 7 реальных executable-сценариев и 2 handler-теста PASS; Python collector: 23 tests PASS. Подробности и команды сохранены в execution workspace и новых evidence.
- 8 компилируемых negative controls завершились ожидаемым runtime FAIL: пропуск full parse, full lock, преждевременный commit, incremental CLI dispatch, inline drain, неверный error kind, пропуск проверки input, live reread inspect. Исходники восстановлены byte-for-byte; covering suites GREEN.
- Фиксация pre/post commit SIGINT, WAL reader, ожидающий publisher и late SQL fault использует реальные процессы и acknowledgement checkpoints. Старый/новый state сравнивает revision, nodes, edges, files и FTS tokens независимым observer.
- Git checkout/reset: добавление/удаление tracked-путей, изменение bytes по тому же пути, tracked ignored и ignored untracked. Источником остаётся обычная Git candidate enumeration; watcher/hooks/HEAD registry не добавлены.
- Удаление cache.db: status не создаёт кэш; reindex ждёт graph/FTS, сохраняет vectors.db и повторно присоединяет прежний vector. Удерживаемый embed.lock и отсутствие serving provider не мешают завершению.

## Финальная host-приёмка

`make verify V=1` завершился успешно на фактически изменённом продукте: **762 passed / 0 failed / 5 ignored** в debug и столько же в release, 54 executable + Doc-tests на профиль, 0 warnings. Три ignored child entry исполняются родительскими schedules; прежний реальный 60s provider deadline и observe_existing_corpus здесь не запускались.

Исходный host runner после этого остановился на collector: status с revision был принят за reindex. Это ошибка Python evidence-классификации; она воспроизведена новой регрессией и исправлена, все 23 Python-теста GREEN. Исходный FAIL receipt сохранён. В новом `.artifacts/tk25/host-continuation-20261026/` полностью выполнен CLI-путь: 51 command records, из них ровно один явно отмечен reuse успешного make verify. До reuse сверены все 221 входа: отличаются только `tests/cli/run.py` и `tests/cli/test_runner.py`, прочие 219 byte-identical; исходные root stdout/stderr скопированы с provenance, повторный запуск не выдается за факт. После continuation все 221 текущих hash совпадают с manifest.

Continuation PASS: 15 injected handler records, 12 actual reindex, 15 reading, 9 transport и 7 thin-slice ответов валидированы по собственным X-1 command schemas. Сохранён executable SHA256 `9ac112d080652d3731caf8b728625fa582e1097a70ff9eb46ccdfe34ecef2bbd`. Дополнительный actual-binary probe на этом же SHA доказывает сохранение непустого авторского source_snapshot и hash невалидного A, публикацию изменённого B и неизменность Git index/HEAD.

## Архитектура

`Refresher` — application interface; `DocumentRefresher` — единственный владелец orchestration. Полный/incremental выбор приватный, projection и source reconciliation общие. Storage schema/implementation, domain и runtime зависимости не менялись. CLI parse/dispatch/format отдельно, handler вызывает capability; реальные constructors находятся в lib.rs. Дополнительный pass-through сервис и bundles аргументов не появились. Touched функции проверены на предел 60 строк, используются typed errors и существующие Rust/Clippy правила.

## Evidence и пределы

Машинные результаты: [verification.json](verification.json). Новые логи: `.artifacts/tk25/core-mutations/`, `cli-mutations/`, `host-final-20261026/` и `host-continuation-20261026/`; execution ledger: `.superpowers/sdd/2026-10-26-tk-25-reindex/progress.md`. Исторические receipts не перезаписывались. Host receipt фиксирует captured HEAD и dirty inputs отдельно; окончательная сверка их hashes с коммитом выполняется после commit.

TK-25 не реализует физическую corruption recovery (TK-26), весь embedding lifecycle (TK-27) или handled cancelled JSON во всех waits (TK-29). Нативный SIGINT здесь доказывает rollback при смерти процесса. Linux TK-25, платные модели/пилоты, RAM/gates не объявляются проверенными. Авторские Markdown/source_snapshot, Git index/HEAD сохраняются. Обновлены только действующие инструкции агента и evidence collector; прежний stale inspect control восстановлен на текущий код и заново mutation-checked.

## Решения исполнителя

Все Ruling строки execution ledger также находятся в verification.json. Они фиксируют сохранение workspace/evidence, повторное использование test fixtures, изменение first-build контрольных точек после переноса capture под lock, отсутствие дублирующего make check только ради wrapper, восстановление stale negative-control anchor, замену удалённого Python окружения и единственный final make verify внутри host runner и прозрачное продолжение после исправления Python collector без повторного Rust-прогона.

## Итоговое независимое review

Fresh reviewer `/root/tk25_final_review` (gpt-6-astra/high) проверил весь диапазон
`913b2b7..0dfe83c`: **Ready to merge: Yes**, Critical/Important/Minor — 0.
Самостоятельно подтверждены 221 committed input hash, две Python-разницы между
фазами, byte-identical reuse успешных root logs и чистота checkout. Broad tests
reviewer не перезапускал. Продуктовых исправлений не запрошено.

Все пять критериев карточки выполнены с указанными пределами. Final metadata
closure меняет только документы; новые Rust-прогоны для него не требуются.
