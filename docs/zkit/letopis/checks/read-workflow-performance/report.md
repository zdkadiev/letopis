# Оптимизация чтения и аудита снимков — выполнение

2026-10-29. Статус: PASS. Реализация, E2E всех команд, 27 инвариантов, mutation checks, release profiling и host/native verification завершены.

Изолированный worktree: `/Users/zdkadiev/.codex/worktrees/a236/letopis`, ветка `perf/read-snapshot-verification`, base `2c134fab9ee3d069f27f387f6468cd6e897e08a0`. Основной checkout и его knowledge не изменялись.

## Поведение

- Search/traverse/inspect сохраняют автоматическое обновление БД из Markdown. Корни/refs и адресованные объекты проверяются; несвязанные descendants не обходятся.
- Полный check (нет paths или пустой список) и reindex проверяют все основания результирующей валидной проекции до SQL. Непустой paths остаётся scoped. Невалидный Markdown сохраняет last-valid roots, валидная замена/удаление убирает старые.
- Полный check сверяет audited revision с pinned read revision; публикация в промежутке даёт safe internal_error/retry. Поздняя публикация после pin не меняет view.
- Сильный pin каждый раз проверяет closure. Один cat-file batch потоково проверяет SHA-1 с collision detection или SHA-256; один hash-object --stdin-paths сохраняет прежние строгие структурные проверки Git по приватному временному spool tree metadata. Blobs не записываются в spool.

## Проверки

Focused: Git unit tests, strong/light retention, corruption, source/knowledge inspection, scope/race, pre-SQL ordering, freshness, SIGINT/EOF. Новый E2E `tests/read_verification.rs` исполняет init/check/search/traverse/inspect/reindex/status/config, help/version; добавления/правки/удаления; unrelated/addressed corruption; новые основания; invalid last-valid; замену/удаление устаревшего основания; отказ публикации до SQL. Старые suites проверяют provider/fallback/config, источник/диапазоны и конкурентные публикации.

15 hand mutants пойманы: OID, hash, trailer, closure traversal, выбор scope, отсутствие audit, revision guard, prospective roots, отсутствие Git structural validation, отсутствие stderr drain, shortcut по existing ref, обход проверки process count, права spool, cleanup spool, C-quoting путей. Файлы восстановлены; focused GREEN после восстановления. Сырой журнал: `.artifacts/read-optimization/`. Внешний watchdog ограничивает блокирующие batch tests и завершает их собственные process groups.

Независимое ревью: Critical нет; Important — nested malformed trees обходили прежний hash-object/fsck. Регрессия воспроизведена RED и исправлена батч-валидатором Git. Lifecycle proof gaps (ранний BrokenPipe, отсутствие deadline) закрыты. Дополнительно восстановлена приватная ExitStatus cause для batch failure. Итоговые полные проверки пройдены. Неисправленных Critical/Important нет; deferred minor findings нет.

## Производительность

Одинаковые base corpus, requests, cache projection revision 1 и release build settings (`CARGO_PROFILE_RELEASE_DEBUG=1`, Rust 1.95.0). FTS, embeddings disabled. Авторское основание содержит 1961 уникальных tree/blob objects. Before: median search 57.940 s, traverse 62.735 s, inspect 75.001 s, по три warmed вызова. Git subprocess counts отдельного wrapper-run: 6239 / 6239 / 6350; full check 6253, reindex 6242.

Ограничения измерения: первый before-run каждой команды одновременно имел 3-секундное sample-окно; на хосте параллельно выполнялись сборки/тесты разработки. Candidate latency измеряется без sampler; corpus/build settings совпадают, host load не изолирован. Это локальные наблюдения, не SLO или строгий изолированный microbenchmark. Инструментированные wall times не смешиваются с обычной latency. Новая cache копия снимается через SQLite backup для одинаковой публикации; provider inference в замер не входит.

| Команда | Before median, s | After median, s | Наблюдаемое ускорение | Git calls before → after |
| --- | ---: | ---: | ---: | ---: |
| search | 57.940 | 0.679 | 85.3× | 6239 → 66 |
| traverse | 62.735 | 0.641 | 97.8× | 6239 → 66 |
| inspect | 75.001 | 1.256 | 59.7× | 6350 → 128 |

Диапазоны по трём warmed вызовам, before / after, секунды:

- search: 56.712–64.013 / 0.660–0.754;
- traverse: 62.619–64.623 / 0.625–0.667;
- inspect: 64.204–82.643 / 1.251–1.279.

Девять пар before/after response совпали по IDs, edges, lines, hashes и flags; snapshot/revision исключены из сравнения как транзиентные поля. Счётчики получены отдельно от latency. Full check: 6253 → 81 Git calls; reindex: 6242 → 71. Отдельные wrapper wall times: check 87.339 → 1.034 s; reindex 80.413 → 1.125 s. Не смешивать эти инструментированные числа с median обычных reads.

Sub-second цель достигнута search/traverse; inspect остаётся 1.26 s. Повторный Rust sample показывает оставшееся время в `InspectionService::assemble_tree` → `ObjectDb::create_tree_from_blobs` и one-shot `verify_object`/Git waits для фактически захваченных blobs. Это уже не обход авторской repository-wide closure. Дальнейшая оптимизация этого отдельного capture path не включена; проверки не отключались ради цифры. CPU-доля всего вызова из короткого sample-окна не выводится.

After бинарник SHA-256: `2b2d6f41742007f6e16389cc116597ca1ab9160233112fa21eb4dd667b800aac`. Verified source commit: `8d3a406`. Raw: `.artifacts/read-optimization/{before-profile,after-profile,cache-baseline,run-context.json}`. Исторический dev-build profile не переписан и не используется вместо release baseline.

## Принятые уточнения выполнения

1. T2–T4 собраны одним локальным implementation commit: промежуточный неиспользуемый production batch module нарушал бы deny dead_code. Цена — более крупная единица review/rollback.
2. Hashing и raw-tree decoder выделены в небольшие приватные модули: отдельные ответственности и короткие функции. Цена — два дополнительных файла навигации.
3. Структурная проверка оставлена Git через один дополнительный batch и временный private spool вместо переноса сложных HFS/NTFS/fsck правил в Rust. Цена — transient disk I/O и дополнительный процесс на closure. Нормализация decoder не разрешает strong pin принимать noncanonical modes, которые отвергал прежний Git.

Обычный success/error/drop удаляет spool; аварийный process exit может оставить приватные tree metadata, как оставляет незакреплённые Git objects. Изменений signal lifecycle, daemon или автоматического cleanup сервиса нет. Полная проверка фиксирует наблюдаемую целостность, не запрещает последующее физическое повреждение object store.

## Итоговые gates и пределы

- `make check`: отдельный PASS до review correction; финальный `_check` (fmt/check/Clippy/debug tests) прошёл повторно внутри `make verify V=1` после исправления.
- `make verify`: PASS через host runner, 926 passed / 0 failed / 11 ignored в debug и столько же в release; строгий Rustdoc PASS. Девять ignored — приватные child entrypoints, вызываемые parent-тестами; existing-corpus entrypoint также проверен parent-тестом. Реальный 60s deadline выполняется отдельно runner-ом.
- `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target CARGO_PROFILE_RELEASE_DEBUG=1 python3 -B tests/storage/run.py --platform host --evidence .artifacts/read-optimization/native-host-20261029`: PASS, все 16 команд exit 0. Schema parity, 10 Python runner tests, native SQLite/vec1/FTS, copied standalone executable, linkage и HTTP deadline 60.01 s — PASS. [Receipt](../../../../../.artifacts/read-optimization/native-host-20261029/manifest.json). Все 408 hashed inputs совпадают с итоговыми файлами.
- Host: macOS ARM64, Git 2.50.1. Linux и фактический запуск Git 2.29 в этом выполнении не проверялись. Исторические отдельные proof harnesses не объявляются повторно пройденными.

Связи: [spec](../../../../superpowers/specs/2026-10-29-read-workflow-performance-design.md), [plan](../../../../superpowers/plans/2026-10-29-read-workflow-performance.md), [27 инвариантов](invariants.md), [исторический baseline](2026-10-29-baseline.md).

Ветка сохранена для review; merge/push не выполнялись. Финальные ручные мутации восстановлены побайтно: повторные Git unit30 и corruption3 — PASS; source/native input hashes совпадают.
