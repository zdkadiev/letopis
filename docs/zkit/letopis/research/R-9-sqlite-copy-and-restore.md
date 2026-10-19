# R-9 — согласованная внешняя копия SQLite и восстановление

Дата: 2026-10-19. Узкая проверка первичных источников основным агентом, без backup/restore эксперимента или измерений.

| Факт | Источник | Confidence / предел |
|---|---|---|
| File-copy во время активной транзакции может смешать состояния. Копирование без транзакций допустимо; после сбоя существующий WAL/hot journal должен сохраняться вместе с соответствующей БД. Подмена файла под открытыми соединениями опасна | [How to corrupt, §§1.2–1.4,1.7](https://sqlite.org/howtocorrupt.html) | high для документированного поведения, не проверка letopis |
| WAL — часть durable состояния SQLite, его нельзя произвольно отделять от main DB; shared-memory WAL index восстанавливаем | [SQLite WAL](https://sqlite.org/wal.html) | high; работоспособность конкретной FS/build остаётся DR |
| Backup API создаёт согласованную SQLite copy; это не общий механизм координации остальных файлов приложения | [SQLite backup](https://sqlite.org/backup.html) | high для SQLite; последнее ограничение — вывод для структуры letopis |

## Значение для letopis

D-80 выбирает внешний operator-owned offline backup. Самый простой coherent baseline — остановить все writers/worker и запретить autostart/внешний Git на время копирования, сохранить весь согласованный набор Markdown/Git/private refs/SQLite с имеющимся WAL/journal. Live SQLite backup сам по себе не обеспечивает соответствие Markdown/Git. Это вывод из принятых границ приложения, не утверждение документации SQLite о нашем протоколе.

Restore выполняется offline в изолированное место. Не смешивать файлы разных копий; после проверки durable реестра/objects normal recovery завершает незавершённую правку до обслуживания. Отсутствие usable copy означает отсутствие конечной гарантии полного restore, как принято D-81/D-82. Внешний tool не выбран, новый продукт/CLI/API не добавлены; DR-2/P-3 проверит всю цепочку на чистом устройстве. Никаких новых failure-rate/RTO/RPO чисел из этих sources не выведено.

## Используется в
<!-- собирается link_docs.py, не править вручную -->
- [resilience.md](../resilience.md): Внешняя согласованная copy/restore-процедура, Sources
- [roadmap.md](../roadmap.md): Sources
