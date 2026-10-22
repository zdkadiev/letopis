# TK-8 — независимое ревью и решения

Проверена ветка от `2a7e8db0758e3410b5ec4327848b7c06185907e4`. Конечный продуктовый код — `92cf8bd72708e9c91c506b3e342c14bc1ea18b78`, evidence — `b344c0871294c69ceadf1ea3031c165eb914ab24`.

1. Task review, gpt-6.1-sol/high: одно Important — удаление всех LF из корня могло направить запись в соседний repository. Реальный RED/GREEN, fix `fa1fff5`; scoped re-review: ADDRESSED, новых замечаний нет. [Отчёт](task-review.md), [проверка исправления](task-rereview-round-1.md).
2. Whole-branch review, gpt-6-astra/high: одно Important — alternate object store позволял GC донора удалить blob принятого снимка. Реальное воспроизведение SHA-1/SHA-256, fix `92cf8bd`; единственная scoped re-review: ADDRESSED, новых Critical/Important/Minor нет. [Отчёт](final-review.md), [проверка исправления](final-rereview.md).

Открытых, отложенных и припаркованных findings нет. Проверяющие работали read-only и не повторяли готовые suites; whole-branch reviewer выполнял только целевое воспроизведение в /tmp. Самопроверка исполнителя не выдаётся за независимое ревью.

## Решения координатора

- Отказывать при наличии любого `objects/info/alternates`, включая пустой/неактивный путь, при открытии и каждой операции. Это сохраняет гарантию без нового парсера, копирования/repack и раскрытия donor paths. Цена: shared/reference clone требует самодостаточной копии; даже неактивную alternates-конфигурацию нужно устранить отдельно. Адаптер её не меняет.
- Корни checkout с не-UTF-8 именами остаются явно неподдержанными и дают безопасную ошибку. Цена: такой каталог нужно переименовать либо отдельно реализовать поддержку произвольных byte paths.

Остальные исключённые reviewer области совпадают с явно отложенными требованиями: полный capture/SQL/CLI/source-сценарии, atomic live capture, power loss/TK-38, реальный Git2.29, x86_64, cancellation/TK-29, ресурсы, враждебный same-UID writer, multi-worktree product semantics, repository-wide fsck и повтор старых harnesses. Они не объявлены PASS.
