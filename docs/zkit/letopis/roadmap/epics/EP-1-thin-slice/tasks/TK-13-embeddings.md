# TK-13 — Реализовать config и базовый embedding executor

> **Статус:** done
> **Эпик:** [EP-1](../epic.md)
> **Фаза:** PH-1
> **Оценка, ч:** 4 / 6 / 12 (best / expected / worst)
> **План:** неделя 9 — 6 ч
> **Зависимости:** [TK-3](../../EP-8-proofs/tasks/TK-3-vector-harness.md), [TK-6](../../EP-8-proofs/tasks/TK-6-provider-harness.md), [TK-9](TK-9-storage.md)


## 1. Цель

Сквозной путь должен соединить реальные адаптеры; ранние RAM и usefulness результаты нужны до расширения полного функционала. Конкретный результат этой задачи: базовый embedding runtime и drain по текстам без вектора.

## 2. Контекст и входные данные

- **Структура программы и границы модулей:** [обязательные архитектурные правила](../../../../architecture.md#program-structure), включая [порядок проверки](../../../../architecture.md#program-structure-review).

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-3, B-3, X-2.
- Покрывает: [FR-22, FR-29, FR-30, NFR-6](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-2](../../../../contracts/X-2-embeddings.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-0. Текущий G-1 закрывается в TK-17 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Читать конфигурацию provider/model/parameters/max_retries из `.letopis/config.json` (правится как файл, D-95) и env-only secret.
2. Реализовать один drain и общий порт texts→vectors для документов/запросов.
3. Выбирать работу запросом (search_hash без вектора и без исчерпанных попыток), перечитывать файл и считать вектор только при совпавшем search_hash; вычислять вне write transaction; записывать вектор в vectors.db по search_hash (MC-3).

## 4. Ожидаемый результат

Базовый embedding runtime и drain по текстам без вектора (D-95).

## 5. Критерии приёмки

- [x] Config из `.letopis/config.json`, env key и один drain используют общий texts→vectors; невалидный файл даёт диагностику и FTS-only.
- [x] Вычисление вне DB write; вектор записан в vectors.db идемпотентно по search_hash; правка во время вычисления пропускается по search_hash; таблицы заданий и seq нет (D-95).

- [x] Соблюдены [структура программы и границы модулей](../../../../architecture.md#program-structure); в отчёте названы проверенные границы/файлы и evidence либо обосновано отсутствие изменений продуктового Rust-кода.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

**Архитектурная проверка:** выполнить [общий порядок проверки структуры](../../../../architecture.md#program-structure-review) для изменяемых компонентов и зафиксировать результат в отчёте; область без изменений продуктового Rust-кода указать явно.

## 7. Границы

- **Входит:** Базовый embedding runtime и drain по текстам без вектора. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Полная замена config/retries/failed — [TK-27](../../EP-3-maintenance/tasks/TK-27-embedding-lifecycle.md); rank — [TK-12](TK-12-search.md); реальная сквозная интеграция — [TK-14](TK-14-thin-slice.md).
- **Интерфейсы — вход:** [TK-3](../../EP-8-proofs/tasks/TK-3-vector-harness.md): Регрессионные сценарии vec1/read view и пропуска устаревшего текста по search_hash; [TK-6](../../EP-8-proofs/tasks/TK-6-provider-harness.md): Boundary harness двух API с воспроизводимыми негативными transport-сценариями; [TK-9](TK-9-storage.md): Начальная схема SQLite и storage adapters для всех согласованных сущностей
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-14](TK-14-thin-slice.md), [TK-16](../../EP-7-early-validation/tasks/TK-16-early-memory.md), [TK-25](../../EP-3-maintenance/tasks/TK-25-reindexing.md), [TK-27](../../EP-3-maintenance/tasks/TK-27-embedding-lifecycle.md), [TK-41](../../EP-4-system-validation/tasks/TK-41-provider-drill.md).

## 8. Риски и примечания

**Основание оценки 4 / 6 / 12 ч (пересчёт 2026-10-20):** после D-95 нет таблицы заданий и seq: drain выбирает тексты без вектора, config читается из файла; best — пробы transport/vec1 TK-3/TK-6 и storage TK-9 переиспользуются; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — стыковка HTTP, общего query/document порта и выборки работы без write transaction во время compute. Это инженерный диапазон, не измеренная скорость.

**Риск и действие:** Стыковка HTTP, общего query/document порта и выборки работы без write transaction во время compute. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## 9. Отчёт о выполнении

2026-10-22: задача завершена в отдельной ветке `TK-13`/worktree от `bf9b6b9`, по одобренным [спецификации](../../../../../../superpowers/specs/2026-10-22-tk-13-embeddings-design.md) и [плану](../../../../../../superpowers/plans/2026-10-22-tk-13-embeddings.md) через Subagent-driven. Реализованы config/JCS/env-only key, синхронные Ollama/OpenAI providers, короткие guarded storage units, native dimension bootstrap, confined file reads и persistent kernel lock, общий Markdown codec и EmbeddingService с drain/query. Trait-интерфейсы принадлежат service, реализации разделены по ответственностям; lib.rs делает реальные constructor calls.

[Итоговый отчёт и evidence](../../../../checks/TK-13/report.md): исходники `bcd405c0d9c6`, macOS ARM64/Linux glibc ARM64 — root219 PASS/2 ignored на каждой платформе, реальный60s тест отдельно1 PASS, runner9/strict Rustdoc/native/standalone PASS;102 общих SHA256 совпадают. Пять real-process parent cases проверяют lock/crash/reservation/hash/config/query races; обе compiling guard mutations выявлены по сохранённому stale vector, точные байты восстановлены, те же проверки GREEN. Root totals включают два старых inert child entrypoints; child executions и ignores учтены отдельно. Исходные отчёты8d39 сохранены исторически, промежуточныеca8 не считаются итоговой приёмкой.

Все пять task reviews и итоговое broad review + общая волна fixes + scoped re-review завершены. I1/I2, M1–M4 и C1 закрыты без новых дефектов: ограничен ответHTTP10MiB, сохранены literal JSON types и unknown config objects, проверяется обязательный native model, исправлены имена/child output/descendant cleanup/zero-request oracle. Шесть решений и их риски записаны в плане, решения/изменения/ревью/проверки — в существующем Engram. Execution ledger сохранён локально в `.artifacts/tk13/execution-ledger-final-aebf196`.

При неготовом embedding библиотека возвращает typed unavailable/diagnostic и сохраняет corpus/graph/FTS/revision; фактический search/ranking — TK-12. CLI запуск — TK-11/TK-14, полный lifecycle — TK-27. TK-10/current main не интегрированы; новые правила порядка кода из main `ce41b48` проверяются при интеграции. Реальные модели/качество/RAM/performance/x86/power-loss/system-TLS drill этой задачей не заявлены. Ветка и worktree сохранены; merge/push/GitHub update ожидают отдельного указания пользователя.
