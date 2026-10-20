# TK-2 — публикация по файлу и прерывание процесса

Дата: 2026-10-21 (Europe/Moscow). **PASS / done** для согласованной матрицы. Ветка `TK-2`, worktree `/Users/zdkadiev/.codex/worktrees/4cf8/letopis`, исходный `main@ea86eada6d45b5fee81b2d74ac5ed659f4d82723`.

## Результат

Создан сохраняемый [Rust integration harness](../../../../../tests/publication/README.md), проверяющий реальные файлы, kernel flock, file-backed SQLite WAL и нативный vec1. Фикстуры и БД создаются во временных каталогах; letopis не меняет корпус или config. Это узкая реализационная проверка механизмов, не production CLI/парсер.

Использованы TK-1 Rust `1.95.0`, rusqlite `0.40.2`, libsqlite3-sys `0.38.2` / SQLite `3.53.2`, vec1 `0.8` / NEON из SQLite commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`. SHA-256 vec1 C: `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`. Источник проверяется до компиляции против заголовка той же locked libsqlite3-sys. Каждый прогон собирает отдельную нативную библиотеку; платформенные Cargo targets разделены.

| Проверка | macOS ARM64 | Linux ARM64 Docker |
|---|---|---|
| TK-2 integration tests | 15 passed, 0 failed/skipped | 15 passed, 0 failed/skipped |
| Реальные SIGKILL cuts | 25/25 PASS | 25/25 PASS |
| Конкурентные процессы | 6 тестов PASS | 6 тестов PASS |
| Полный TK-1 regression runner | 10 Rust + 4 Python PASS | 10 Rust + 4 Python PASS |
| fmt / Clippy `-D warnings` | PASS / PASS | PASS / PASS |
| Roadmap checker | 45 задач, 329 ч, 33 недели; ошибок нет | тот же результат |
| Hashes исходников и consumed inputs | совпадают с финальными файлами | совпадают с финальными файлами |

Шесть конкурентных тестов входят в 15 integration tests; 25 cuts находятся внутри трёх matrix tests и не прибавляются к числу тестов. Linux — реальные процессы внутри Linux ARM64 VM, не cross-compilation и не физический Linux-хост. [Финальная сверка](final-verification.json) содержит image ID, manifest hashes и результат проверки сохранности checkout.

## Матрица прерываний и oracle

Родитель получает flushed checkpoint по stdout и завершает остановленный дочерний процесс SIGKILL. Проверяется завершение именно сигналом 9. Родитель не снимает flock, не откатывает SQL и не чинит кэш: восстановление наблюдается через новое подключение и следующий дочерний вызов.

| Операция | До commit: точный прежний committed state | После commit: полный новый state при потере ответа |
|---|---|---|
| Публикация, 10 cuts | detected, locked, redetected, begun, cleared, projections, fts, fingerprints, before_commit | after_commit |
| Полный reindex, 9 cuts | locked, redetected, begun, cleared, projections, fts, fingerprints, before_commit | after_commit |
| Сброс config, 6 cuts | reset_detected, reset_begun, reset_cleared, reset_identity, reset_before_commit | reset_after_commit |

Публикация/reindex используют old A/B/C, затем invalid A, valid changed B, удалённый C и новый D; старое ребро C→A заменяется B→D. До фиксации сравниваются все поля nodes/edges/files, revision и все FTS token instances с известным исходным состоянием. После фиксации независимо задаются ожидаемые проекции и SHA-256 байтов A/B/D, стат/hash/error текущих файлов, точные рёбра, все FTS token instances и revision; checked_at проверяется как допустимый timestamp. Повторный вызов догоняет изменения, корпус остаётся byte-for-byte прежним. При reindex дополнительно закреплённый WAL reader сохраняет старое представление.

Для reset исходно существуют старая identity, vector key, нативный вектор `[1,0]` и failed attempt. Oracle сравнивает identity, keys, failures, vector bytes и sqlite_master; main cache и корпус/config не меняются. Повторный reset той же identity — no-op. Конкурентный второй процесс сохраняет sentinel vector/failure, добавленные после первого reset; index.lock в этом тесте удерживается отдельно, reset от него не зависит.

## Дополнительные сценарии

- Невалидный файл сохраняет прежние node/FTS и текущую ошибку, валидный сосед обновляется; ошибка исчезает после исправления/удаления. Новые, удалённые и переименованные `.md`/`.markdown` отражаются в FTS; row_id при update сохраняется.
- По принятому D-95 объявленные рёбра хранятся без active-флага: отсутствующая цель скрыта join, возвращение цели делает старое ребро видимым без правки исходного файла; includes-цикл диагностируется, BFS с visited завершается. Прежняя формулировка карточки о запрете публикации цикла исправлена с согласия пользователя.
- Same-tick тест явно задаёт stored racy-clock precondition независимо от setup/scheduling, реально сохраняет равные size/mtime/inode, заменяет `alpha` на `omega` и проверяет новый FTS-результат. Детектор хеширует racy-файлы с консервативным тиком 1 секунда.
- Два издателя синхронизируются pipes; checkpoint waiting достигается только после kernel `flock(LOCK_NB)` → WouldBlock. Проверены две очередности agent edit / commit, повтор одной дельты, смерть держателя flock, reindex рядом с публикацией, конкурентные resets и сохранность векторов при форматировании config.
- У неизменённого вызова нет публикации/роста revision и ожидания index.lock. Родитель ограничивает ожидания 20 секундами и убивает/reap-ит оставшихся детей при ошибке; SQLite busy timeout после открытия — 10 секунд.

## Evidence и воспроизведение

- [macOS manifest](macos-arm64/manifest.json), [15 Rust tests и 25 cuts](macos-arm64/07-cargo.txt), [TK-1 regression](macos-arm64/tk1-regression/manifest.json).
- [Linux manifest](linux-arm64/manifest.json), [15 Rust tests и 25 cuts](linux-arm64/07-cargo.txt), [TK-1 regression](linux-arm64/tk1-regression/manifest.json).
- [Docker build](docker-build.log): тот же pinned Rust base digest, Debian build packages, rustfmt/clippy именно toolchain 1.95.0, Linux image ID в final-verification.json.
- RED до реализации: [публикация](task1-red.log), [диагностика цикла](task1-graph-red.log), [process checkpoints](task2-cuts-red.log), [reset mode](task2-reset-red.log).
- Четыре компилирующиеся мутации обнаружены: [нет redetect](mutation-redetect-red.log), [fingerprints вне основной транзакции](mutation-early-fingerprints-red.log), [нет reset identity recheck](mutation-reset-recheck-red.log), [нет racy hash](mutation-racy-hash-red.log). [Hashes восстановленных исходников](mutations.json), [GREEN после восстановления](mutation-restored-green.log). Diff мутации и упавшие assertions сохранены; библиотечные исходники восстановлены byte-for-byte и сверены с финальными manifests.

Точные команды для host и Docker приведены в [README](../../../../../tests/publication/README.md). Host:

```sh
python3 -B tests/publication/run.py --evidence NEW_DIRECTORY
python3 -B docs/zkit/letopis/roadmap/schedule.py --check
```

Первый Linux-прогон остановился **до компиляции**: crates.io sparse-index `ryu` вернул network timeout ([FAIL manifest](linux-network-failure/manifest.json), [ошибка](linux-network-failure/04-cargo.txt)). Повтор с read-only host Cargo registry и offline Cargo собрал и прошёл все 15 Linux-тестов, затем обнаружил отсутствие rustfmt в базовом image ([FAIL manifest](linux-tooling-failure/manifest.json)). Для финального прогона image дополнен pinned rustfmt/clippy на этапе build; offline-параметры и точные argv записаны в Linux manifest. Host source cache содержит только исходники/registry metadata, Linux заново компилирует свои бинарники. Эти промежуточные неуспехи не считаются пройденными проверками.

## Независимое ревью

[Reviewer report и исправление](review.md): Critical 0, Important 1, Minor 0. Найдена зависимость same-tick fixture от скорости первичного setup. [Детерминированный RED без sleep](review-racy-window-red.log) подтвердил причину; тест теперь явно задаёт и проверяет отношение stored checked_at/mtime. [Полная GREEN suite](review-racy-window-green.log), повторная racy-мутация и новые полные platform runs прошли. Открытых material findings и отложенных Minor нет; повторное независимое ревью не заявляется.

## Решения исполнителя и границы

1. Изменения оставлены незакоммиченными согласно плану; ревью получает явные TK-2 пути, поскольку copied design baseline не принадлежит этой задаче. Стоимость: commit/merge позже должен отделить TK-2 от пользовательских правок.
2. Reindex перечитывает все файлы, заменяет все валидные проекции и удалённые элементы одной unit, но сохраняет прежнюю проекцию невалидных файлов, включая contentless FTS. Это MC-1, а не доказательство восстановления повреждённого кэша; такое восстановление остаётся TK-26.
3. Config identity проверена на string/integer-only fixture subset через sorted JSON, не на общей JCS-грамматике. Полное config/schema/JCS соответствие не доказано.
4. Для Linux после network timeout использован read-only исходный Cargo cache и offline native build без смены pins. Cold-cache сборка по-прежнему зависит от доступности registry.
5. Добавлен TK-2 Dockerfile с тем же pinned Rust base и Debian-пакетами, но установленными rustfmt/clippy для offline запуска. Дополнительный слой image не меняет runtime/dependency pins.

Фикстуры используют один file node, узкую JSON-comment грамматику, фиксированные вспомогательные метаданные и короткие ASCII-тексты; это не X-4/parser validation (TK-7), Git/provenance (TK-8) или production storage/publication (TK-9/TK-10). Проверен тик FS 1 секунда; более грубые timestamps и намеренное восстановление старого non-racy mtime не покрыты. Произвольная конкурентная запись в live-файл не является атомарным snapshot; наблюдаемое изменение metadata во время чтения возвращает ошибку.

Проверки не выбирают инструкции внутри SQLite COMMIT и не моделируют физическое отключение питания (TK-38). ATTACH, смена размерности vec1 со старым reader, drain/attempts и late embedding results — TK-3. G-0 ещё не закрыт. Исходный checkout сохранён: сверены hashes всех 303 исходных файлов; изменения copied baseline ограничены собственными TK-2 статусом/отчётом и D-95 wording. Новые artifacts принадлежат TK-2.

## Последующий запрос commit/merge

По запросу пользователя TK-2 коммитится отдельно от пользовательского design baseline и интегрируется в `main` (ветки `master` в репозитории нет). Схема, ранее подключавшаяся из незакоммиченного design-документа, сохранена byte-for-byte в `tests/publication/schema.sql`; include и manifest используют этот самостоятельный fixture. Обе platform suites повторно прошли после упаковки; самостоятельный staged snapshot также проходит полный host runner и проверку опубликованного расписания. Содержательная карточка TK-2 следует принятому локальному D-95 snapshot; при коммите её плановые поля сохраняют опубликованные часы/недели до отдельной публикации общего пересчёта. Остальные design-правки и их staged state сохраняются.
