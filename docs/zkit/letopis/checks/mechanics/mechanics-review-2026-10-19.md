# letopis — mechanics: проверка и независимое ревью

Дата: 2026-10-19. Применены zkit:flow, zkit:mechanics и узкая проверка первичных источников для storage primitives. Resource probes/inference/production-код/коммиты не запускались. Временный Mermaid parser установлен вне проекта, install scripts отключены; npm user cache не изменялся.

## Интервью и границы

Фиксированный R1 T=3 закрыт 3/3, в очереди 0. D-76 — одна защищённая фаза сохранения/восстановления, D-77 — Git snapshot для согласованных multi-document reads без гарантии для ordinary live reads, D-78 — max_retries в существующей конфигурации. D-79 — явные assumed инженерные starting choices, не выдуманные пользовательские решения. Whole config replacement D-69 сохранён, включая переустановку очереди/векторов при config --set; особый retry-only update не добавляется.

Изменены существующие config schema/examples и SQL, а не добавлена новая команда/таблица config. Max_retries — число дополнительных попыток, 0 означает initial only. Attempt reservation до HTTP выражена одним attempts вместо эскизного retries; conservative crash может потратить слот без вызова, но не обходит предел. Query embedding остаётся прямым вызовом с FTS fallback.

## Артефакты

Mechanics: 7 MC, 4 state-machine tables, 3 executable stdlib models. Six internal contract files покрывают существующие I-1…I-12; внешних границ по-прежнему 4. SQL — физический проект, не исполнявшаяся миграция. Git/Markdown — единственные bytes; nodes.search_hash производный метахеш, не текстовый cache. Source identity, config, errors и recovery не объявлены пересоздаваемым кэшем.

NDJSON Unix socket — внутренний транспорт D-74, внешний stdin/stdout остаётся прежним JSON. Один worker/kernel owner и gate не превращают подготовку агента в длительный пессимистический lock. C4 level 4 — диаграмма важных типов/портов, синтаксически разобрана Mermaid 12.2.0 classDiagram перед включением в Markdown; визуальная проверка/production class design не заявляются.

## Независимый review

Свежий read-only reviewer mechanics_spec_review прочитал входы, контракты, новые модели и SQL. Первая сверка: пять содержательных замечаний, без новых продуктовых вопросов.

1. **Source basis retention:** принимаемый source_snapshot может не совпадать с before/after. Добавлен отдельный pin+harden каждого basis tree и выдаваемого inspect tree, плюс GC counterexample модели.
2. **Reindex SIGINT:** восстановлена согласованная X-1 семантика отмены общей связанной перестройки для всех waiters; проверены cancel до/после commit.
3. **Memory:** Vec всех search texts заменён адресами/hash и последовательным чтением одного текста из immutable tree внутри публикации. Полный reindex может дольше держать writer/gate; короткая фиксированная задержка не обещается.
4. **Commit model:** добавлены volatile/durable/dirty bytes, реальный flush, crash images и journal removal в том же автомате. Добавлены snapshot/live reader interleavings и отдельные canonical-target/base-revision/config guards reindex. Broken не создаёт искусственное заведомо плохое состояние вне протокола.
5. **Runtime model:** реальные BFS transitions owner/bind/recover/serve/crash, durable journal/diagnostic flags, staged SQL completion и crash cuts вместо заранее присвоенных правильных исходов.

Повторный review подтвердил первые пять исправлений, выявил ещё один lifecycle gap: единственная error_id могла потеряться после удаления reindex row либо замены retry ошибки. **Шестое исправление:** единый приватный errors.owner вместо error_id в jobs/reindex_state. Владелец находит active по reindex/embedding:<uuid> после рестарта; retry одной unit завершает прежнюю active и добавляет новую, сообщения остаются до clear. Добавлены model permutations и отдельная мутация diagnostic_owner. Это не публичный ID операции, новый реестр, агрегация или дедупликация ошибок.

Финальная точечная сверка reviewer: последний блокер закрыт, новых actionable findings нет. Reviewer независимо выполнил три normal модели и все 14 isolated guard removals: результаты подтверждены. Логическая mechanics clarity gate может быть закрыта.

## Проверки

Полный протокол: mechanics-checks.json. Модели/отдельные защиты/internal schema cases: mechanics-models.json. Внешние формы: 67 CLI examples, 6 provider examples, 8 metadata examples, всего 97 assertions, errors=[]; внутренние формы: 69 cases, 3 модели и 14 isolated protections. Ранее сохранённые G1/G2/intake отчёты — история, не текущие результаты. Скрипты requirements/resource-math/contracts/data/architecture/mechanics и linker проверяют структуру/даты, не реализацию; прежнее объяснённое предупреждение о 95% Must остаётся.

## Что не проверено

Модели конечные: два конкурирующих актёра, перечисленные crash points и guards. SQLite atomic commit, kernel lock и DurableTree — предпосылки, а не доказанная моделью реализация. Реальный file/directory/macOS full flush, Git objects/refs, power-loss, vec1 schema reset с pinned readers, multi-process signals/permissions и full DB loss — P-3/A-AR. Полная RAM на 4 ГБ устройстве, 1 ГБ приложения+модель, exact search/detection scan latency, качество retrieval, реальная очередь и годовой диск — P-1/P-2. Resource-math остаётся draft/not assessed; новых измерений нет.

## Итог

Mechanics — agreed в пределах детального логического проекта с явными assumed choices и именованными проверками. Все шесть substantive findings разобраны и устранены. Следующий этап — resilience; он этим запуском не начинается. Реализация и ресурсные/аварийные гарантии не объявляются выполненными.
