# TK-8 — итоговая приёмка

Дата: 2026-10-22 (Europe/Moscow). Статус: **done в ветке TK-8**, готова к выбору способа интеграции. Main/remote/GitHub issue этим статусом не изменены.

Реализован `GitSnapshotPort` с create_tree/read_blob/pin, конкретный private GitCliAdapter и явная сборка в lib.rs. Физические границы service/ports, adapters/git/process|objects|refs соблюдены; domain/parser не менялись. Git вызывается непосредственно через argv/stdin, без shell/runtime/new dependencies и изменения пользовательского index/HEAD/config/corpus.

## Проверки и evidence

- **53 tests PASS**, fmt/Clippy `-D warnings` PASS: macOS ARM64 с Git2.50.1 Apple и Linux glibc ARM64 с Git2.39.5. Linux собирает source и выполняется `--network none`. [Актуальный платформенный отчёт](final-fix-wave/report.md), [receipt](final-fix-wave/verification.json).
- Обе object formats: SHA-1/SHA-256. Точные bytes, immutable reads, literal paths, prewrite validation, missing/type/corrupt distinction, verified candidate closure, direct-ref CAS, symbolic/conflicting refs, concurrent pin, no-fetch/gitlink, retained snapshots GC/reopen и реально собранный GC negative control.
- Координатор после чистого scoped final review выполнил `cargo +1.95.0 test --locked --offline`: **53 PASS/0 failed**, [полный лог](controller-final-test.log). Исходники после платформенной проверки не менялись.
- Оба независимых Important исправлены с behavioral RED/GREEN; открытых findings нет. [Ревью и решения](review.md), [журнал](execution-ledger.md), [полный отчёт исполнителя](execution-report.md).
- Проверка расписания/локальных архитектурных ссылок и окончательные SHA-256 записаны в `completion-verification.json`. Исторические receipts/logs не переписываются и относятся к своим commits. Последний receipt захватил spec до финального обновления статуса и scratch SDD report; его точные bytes сохранены в Git/`execution-report.md`, relocation описана в completion receipt.

## Принятые ограничения

Любой существующий `objects/info/alternates`, включая пустой/неактивный путь, даёт явный отказ при open/create/read/pin. Shared/reference clone требует самодостаточного object store; адаптер не копирует/repack/чистит конфигурацию. Это защищает pin от обычного GC донора. Корни checkout с не-UTF-8 именами безопасно отклоняются; для них нужно переименование или отдельная поддержка byte paths.

Не доказаны реальное выполнение на Git2.29, power-loss/file-directory/full-flush durability, x86_64, atomic live capture и resource limits. Полные inspect/CLI/SQL/source-сценарии, cancellation и multi-worktree product semantics остаются своими TK.

Продуктовый commit: `92cf8bd72708e9c91c506b3e342c14bc1ea18b78`; платформенное evidence: `b344c0871294c69ceadf1ea3031c165eb914ab24`. Итоговый документационный commit виден в истории ветки; self-referential hash не добавляется.
