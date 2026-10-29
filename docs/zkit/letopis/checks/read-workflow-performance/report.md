# Оптимизация чтения и аудита снимков — выполнение

2026-10-29. Статус: реализация и focused/mutation проверки завершены; итоговые native gates и candidate profiling выполняются. Эта запись не объявляет итоговый PASS до заполнения результатов.

Изолированный worktree: `/Users/zdkadiev/.codex/worktrees/a236/letopis`, ветка `perf/read-snapshot-verification`, base `2c134fab9ee3d069f27f387f6468cd6e897e08a0`. Основной checkout и его knowledge не изменялись.

## Поведение

- Search/traverse/inspect сохраняют автоматическое обновление БД из Markdown. Корни/refs и адресованные объекты проверяются; несвязанные descendants не обходятся.
- Полный check (нет paths или пустой список) и reindex проверяют все основания результирующей валидной проекции до SQL. Непустой paths остаётся scoped. Невалидный Markdown сохраняет last-valid roots, валидная замена/удаление убирает старые.
- Полный check сверяет audited revision с pinned read revision; публикация в промежутке даёт safe internal_error/retry. Поздняя публикация после pin не меняет view.
- Сильный pin каждый раз проверяет closure. Один cat-file batch потоково проверяет SHA-1 с collision detection или SHA-256; один hash-object --stdin-paths сохраняет прежние строгие структурные проверки Git по приватному временному spool tree metadata. Blobs не записываются в spool.

## Проверки

Focused: Git unit tests, strong/light retention, corruption, source/knowledge inspection, scope/race, pre-SQL ordering, freshness, SIGINT/EOF. Новый E2E `tests/read_verification.rs` исполняет init/check/search/traverse/inspect/reindex/status/config, help/version; добавления/правки/удаления; unrelated/addressed corruption; новые основания; invalid last-valid; замену/удаление устаревшего основания; отказ публикации до SQL. Старые suites проверяют provider/fallback/config, источник/диапазоны и конкурентные публикации.

10 hand mutants пойманы: OID, hash, trailer, closure traversal, выбор scope, отсутствие audit, revision guard, prospective roots, отсутствие Git structural validation, отсутствие stderr drain. Файлы восстановлены; focused GREEN после восстановления. Сырой журнал: `.artifacts/read-optimization/`. Внешний watchdog ограничивает блокирующие batch tests и завершает их собственные process groups.

Независимое ревью: Critical нет; Important — nested malformed trees обходили прежний hash-object/fsck. Регрессия воспроизведена RED и исправлена батч-валидатором Git. Lifecycle proof gaps (ранний BrokenPipe, отсутствие deadline) закрыты. Дополнительно восстановлена приватная ExitStatus cause для batch failure. Итоговые полные проверки ещё выполняются.

## Производительность

Одинаковые base corpus, requests, cache projection revision 1 и release build settings (`CARGO_PROFILE_RELEASE_DEBUG=1`, Rust 1.95.0). FTS, embeddings disabled. Before: median search 57.940 s, traverse 62.735 s, inspect 75.001 s, по три warmed вызова. Git subprocess counts отдельного wrapper-run: 6239 / 6239 / 6350; full check 6253, reindex 6242.

Ограничения измерения: первый before-run каждой команды одновременно имел 3-секундное sample-окно; на хосте параллельно выполнялись сборки/тесты разработки. Candidate latency измеряется без sampler; corpus/build settings совпадают, host load не изолирован. Это локальные наблюдения, не SLO или строгий изолированный microbenchmark. Инструментированные wall times не смешиваются с обычной latency. Новая cache копия снимается через SQLite backup для одинаковой публикации; provider inference в замер не входит.

Candidate результаты — pending. Raw: `.artifacts/read-optimization/{before-profile,after-profile,cache-baseline,run-context.json}`. Исторический dev-build profile не переписан и не используется вместо release baseline.

## Принятые уточнения выполнения

1. T2–T4 собраны одним локальным implementation commit: промежуточный неиспользуемый production batch module нарушал бы deny dead_code. Цена — более крупная единица review/rollback.
2. Hashing и raw-tree decoder выделены в небольшие приватные модули: отдельные ответственности и короткие функции. Цена — два дополнительных файла навигации.
3. Структурная проверка оставлена Git через один дополнительный batch и временный private spool вместо переноса сложных HFS/NTFS/fsck правил в Rust. Цена — transient disk I/O и дополнительный процесс на closure. Нормализация decoder не разрешает strong pin принимать noncanonical modes, которые отвергал прежний Git.

Обычный success/error/drop удаляет spool; аварийный process exit может оставить приватные tree metadata, как оставляет незакреплённые Git objects. Изменений signal lifecycle, daemon или автоматического cleanup сервиса нет. Полная проверка фиксирует наблюдаемую целостность, не запрещает последующее физическое повреждение object store.

## Итоговые gates и пределы

- `make check`: PASS до review correction; требуется итоговый запуск по исправленному дереву.
- `make verify`: pending.
- `python3 -B tests/storage/run.py --platform host --evidence <fresh-directory>`: pending; включает make verify, native/standalone/linkage и реальный provider deadline.
- Linux: в этом выполнении не запускался.

Связи: [spec](../../../../superpowers/specs/2026-10-29-read-workflow-performance-design.md), [plan](../../../../superpowers/plans/2026-10-29-read-workflow-performance.md), [27 инвариантов](invariants.md), [исторический baseline](2026-10-29-baseline.md).
