# TK-8 — интеграция в main

Дата: 2026-10-22, Europe/Moscow. Принятая ветка: `TK-8@9f5236cbed87726bc55b12cd5670c93bcba4c54a`. Merge commit: `be77bf56c1daa77e31db7f1cff5cc45619ac540e`; первый родитель — `main@2a7e8db0758e3410b5ec4327848b7c06185907e4`. Этот отчёт добавляет результаты интеграции к исторической [приёмке](../completion.md).

Реализован `GitSnapshotPort`: создание tree из переданных bytes, точное чтение по OID и удержание проверенного object closure через служебные refs. Проверены SHA-1/SHA-256, GC, повреждения объектов, конфликтующие/символические refs, конкурентный pin, запрет lazy fetch и обработка отсутствующих объектов. Git вызывается argv/stdin, без shell; пользовательские HEAD/index/config/corpus не меняются.

Следующие согласованные изменения включены в ту же ветку: импорты типов, именованные константы по владельцу, функции с ориентиром 40/лимитом 60 строк кода, Arrange/Act/Assert в тестах, глагольные имена методов и accessors `get_*`, инструкции `AGENTS.md` и выбранные pedantic lints. Доменные и Markdown-файлы получили изменения читаемости и имён API; утверждение исходной приёмки об их отсутствии относится к прежнему снимку. Каноническая семантика Markdown не менялась.

## Проверки слитого кода

- macOS ARM64: `./scripts/check-rust.sh` — rustfmt, compiler checks, Clippy `-D warnings`, **53 tests PASS**. [Лог](host.log).
- macOS: `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` — PASS. [Лог](rustdoc.log).
- Linux glibc ARM64, Git2.39.5, Rust1.95.0: сборка скопированных исходников, запуск `docker run --rm --network none` — **53 tests PASS**, fmt/Clippy PASS. [Build](linux-build.log), [run](linux-run.log).
- Точные входные SHA-256, merge/base/feature commits и image ID: [verification.json](verification.json). После проверок продуктовый код не менялся; следующий commit содержит только этот отчёт, логи и обновление карточки.

Существующие четыре незакоммиченные правки дизайна в основном checkout сохраняются отдельно от интеграции; проверено чистое трёхстороннее совмещение с tree TK-8. Их содержание в этот merge не включено. Временный worktree используется для проверки merge и затем архивируется.

Принятые ограничения сохраняются: любой alternates path отвергается; корни не-UTF-8 не поддерживаются. Реальный Git2.29, power loss, x86_64, atomic live capture и полные CLI/SQL/source-сценарии не объявляются проверенными. Исторические receipts не переписаны. Итог публикации и состояние задачи фиксируются в [GitHub issue #8](https://github.com/zaurkadiev/letopis/issues/8).
