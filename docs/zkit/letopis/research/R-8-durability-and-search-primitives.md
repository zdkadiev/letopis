# R-8 — документированные примитивы durability и поиска

Дата: 2026-10-19. Тип: узкая проверка первичных источников основным агентом; без измерений, inference или новых библиотечных проб.

| Факт | Источник | Уверенность / предел |
|---|---|---|
| Git core.fsync выбирает hardening компонентов, включая объекты и references; core.fsyncMethod=writeout-only не обещает durability после системного сбоя. Командный успех не заменяет барьер хранилища | [Git config, core.fsync/core.fsyncMethod](https://git-scm.com/docs/git-config#Documentation/git-config.txt-corefsync) | high для документированного API; не доказательство нашего адаптера |
| SQLite WAL даёт читателю снимок; FULL синхронизирует WAL на commit. Долгий читатель удерживает WAL. Внешние Markdown не входят в эту транзакцию | [WAL](https://sqlite.org/wal.html), [isolation](https://sqlite.org/isolation.html) | high; зависит от корректных нижележащих flush/locking |
| Contentless-delete FTS5 поддерживает обычные DELETE/UPDATE, но не хранит исходный текст | [FTS5](https://www.sqlite.org/fts5.html#contentless_delete_tables) | high; индекс не самодостаточен для восстановления текста |
| Vec1 no-index и flat — точные режимы, distance допускает cos; первая вставка определяет длину вектора, последующие другой длины отклоняются | [Vec1 manual](https://www.sqlite.org/vec1doc.html) | high; реальный rollback/config reset и работа со старым read view требуют отдельной пробы |

## Значение для letopis

Durable Git-порт должен завершить hardening объектов/refs до recovery row; default настройки Git недостаточны для заявления о выполненном power-loss требовании. Вызываемые команды получают явные core.fsync=all и core.fsyncMethod=fsync без изменения пользовательского config; адаптер обязан обеспечить платформенные file/directory барьеры, на macOS полный flush. Если примитив недоступен или не прошёл P-3, статус гарантии остаётся неподтверждённым. Модель проверяет порядок при абстракции DurableTree, не устройство диска.

No-index/cos — assumed baseline D-79 без training и premature flat оптимизации. Полная смена конфигурации пересоздаёт vector virtual table в той же SQLite-транзакции, чтобы старая размерность не мешала новой; старые читатели сохраняют read snapshot. Это требование к адаптеру для P-2/P-3, не подтверждённый runtime. Новой ёмкости/скорости из документации не выводим.

## Используется в
<!-- собирается link_docs.py, не править вручную -->
- [mechanics.md](../mechanics.md): MC-2 — commit, rollback и reindex, MC-3 — embeddings, seq и попытки, Sources
- [resilience.md](../resilience.md): 4. Security and isolation design, Sources
- [roadmap.md](../roadmap.md): TK-1, TK-3
