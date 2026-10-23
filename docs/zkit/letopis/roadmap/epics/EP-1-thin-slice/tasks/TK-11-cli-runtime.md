# TK-11 — Реализовать CLI-процесс, блокировки и JSON

> **Статус:** done
> **Эпик:** [EP-1](../epic.md)
> **Фаза:** PH-1
> **Оценка, ч:** 4 / 6 / 10 (best / expected / worst)
> **План:** неделя 8 — 3 ч; неделя 9 — 3 ч
> **Зависимости:** [TK-4](../../EP-8-proofs/tasks/TK-4-runtime-harness.md), [TK-9](TK-9-storage.md), [TK-10](TK-10-change-detection.md)


## 1. Цель

Сквозной путь должен соединить реальные адаптеры; ранние RAM и usefulness результаты нужны до расширения полного функционала. Конкретный результат этой задачи: рабочий CLI без постоянного процесса, с блокировками и запуском drain.

## 2. Контекст и входные данные

- **Структура программы и границы модулей:** [обязательные архитектурные правила](../../../../architecture.md#program-structure), включая [порядок проверки](../../../../architecture.md#program-structure-review).

- План и расписание: [roadmap](../../../../roadmap.md).
- Основание: MC-5, X-1, B-1.
- Покрывает: [FR-3, FR-25, NFR-2, C-1, C-2, C-7](../../../../requirements.md).
- [Требования и критерии системы](../../../../requirements.md).
- [Архитектура и владельцы](../../../../architecture.md).
- [Механизмы и внутренние контракты](../../../../mechanics.md).
- [X-1](../../../../contracts/X-1-cli.md).
- До начала получить результаты указанных зависимостей. Перед началом фазы должен быть пройден G-0. Текущий G-1 закрывается в TK-17 после задач этой фазы, а не требуется заранее. Описание существующего решения приоритетнее предположений о ещё не созданных файлах/командах.

## 3. Что сделать

1. Реализовать процесс на вызов: разбор argv средствами std, тело — JSON из stdin (`serde` с `deny_unknown_fields`), общий JSON envelope, exit codes (D-96).
2. Подключить `std::fs::File::lock`/`try_lock` для index.lock/embed.lock и отсоединённый запуск `letopis embed --drain` (`libc::setsid`, 0/1/2 → /dev/null); TK-4 проверял те же flock-семантики через `libc::flock`.
3. Переиспользовать готовность конфигурации и config identity из [TK-13](TK-13-embeddings.md): SHA-256 (`sha2`) от RFC 8785 (`serde_json_canonicalizer`). Второй config/JCS/provider/executor в TK-11 не создавать. Независимые runtime-проверки не ждут TK-13; приёмка настоящего drain требует его готовой реализации и отдельной проверки объединённого дерева.
4. Реализовать CLI-границу check/inspect: строгие типизированные запросы/ответы, вызов интерфейсов сервисов, отображение ошибок и exit codes. Контрактные тесты обработчиков используют явно обозначенные test doubles; приёмка реального inspect принадлежит [TK-12](TK-12-search.md), полного check — [TK-19](../../EP-2-knowledge/tasks/TK-19-check-renames.md). Успешные фиктивные ответы в продуктовом бинарнике недопустимы.

## 4. Ожидаемый результат

Рабочий CLI без постоянного процесса, с блокировками и запуском drain (D-93/D-94).

## 5. Критерии приёмки

- [x] Граница check/inspect отклоняет невалидные запросы, вызывает интерфейс сервиса и передаёт результат/ошибку в X-1 без прямого вызова адаптеров; evidence с test doubles обозначено как контрактная проверка, а не реальный inspect/check.
- [x] Отсоединённый процесс проходит реальные проверки setsid, fd 0/1/2 → /dev/null, освобождённой probe-блокировки, конкурентного запуска и завершения foreground без ожидания drain.
- [x] После подключения готового [TK-13](TK-13-embeddings.md) настоящий drain запускается, когда есть подходящие тексты без вектора, и завершается, когда работы не осталось; competing drain завершается без второго владельца (D-95). Fixture-процесс не заменяет этот критерий.
- [x] Один JSON stdout, exit 0/1, unknown fields reject.
- [x] Повтор вызова после потери ответа идемпотентен; letopis не меняет корпус.

- [x] Соблюдены [структура программы и границы модулей](../../../../architecture.md#program-structure); в отчёте названы проверенные границы/файлы и evidence либо обосновано отсутствие изменений продуктового Rust-кода.

## 6. Как проверить

1. Выполнить сценарии из разделов «Что сделать» и «Критерии приёмки» на подготовленных входах; для изменения кода запустить соответствующие unit/integration/contract checks созданной реализации.
2. Для каждого критерия записать команду или последовательность действий, ожидаемый исход и фактический результат; сверить отрицательные/аварийные случаи, прямо перечисленные в критериях.
3. Приложить минимальные воспроизводимые evidence: версию сборки, платформу/конфигурацию, входы и результаты. Ещё не существующие CLI/test targets не объявлять запущенными; точные команды появятся вместе с реализацией.

**Архитектурная проверка:** выполнить [общий порядок проверки структуры](../../../../architecture.md#program-structure-review) для изменяемых компонентов и зафиксировать результат в отчёте; область без изменений продуктового Rust-кода указать явно.

## 7. Границы

- **Входит:** Рабочий CLI без постоянного процесса, с блокировками и запуском drain. Работа и собственные проверки перечислены в разделе 3.
- **Не входит:** Реальные read-алгоритмы и приёмка inspect — [TK-12](TK-12-search.md)/[TK-24](../../EP-2-knowledge/tasks/TK-24-inspect.md); полный check и его реальная CLI-приёмка — [TK-19](../../EP-2-knowledge/tasks/TK-19-check-renames.md); config/provider/embedding executor — [TK-13](TK-13-embeddings.md); обработанная отмена SIGINT — [TK-29](../../EP-3-maintenance/tasks/TK-29-cancellation.md); runtime proof уже в [TK-4](../../EP-8-proofs/tasks/TK-4-runtime-harness.md).
- **Интерфейсы — вход:** [TK-4](../../EP-8-proofs/tasks/TK-4-runtime-harness.md): Проверенная координация процессов: одна публикация, один drain, отмена SIGINT с negative fixtures; [TK-9](TK-9-storage.md): Начальная схема SQLite и storage adapters для всех согласованных сущностей; [TK-10](TK-10-change-detection.md): Production путь обнаружения изменений и публикации по файлу
- **Интеграционный вход для приёмки drain:** готовый config/embedding executor [TK-13](TK-13-embeddings.md); это не блокирует независимую реализацию CLI/lock/spawn, но до интеграции соответствующий критерий остаётся открытым.
- **Интерфейсы — выход:** Результат раздела 4 и воспроизводимые проверки получают [TK-12](TK-12-search.md), [TK-19](../../EP-2-knowledge/tasks/TK-19-check-renames.md), [TK-14](TK-14-thin-slice.md), [TK-29](../../EP-3-maintenance/tasks/TK-29-cancellation.md).

## 8. Риски и примечания

**Основание оценки 4 / 6 / 10 ч (пересчёт 2026-10-20):** после D-94 нет worker и IPC-протокола: процесс на вызов с общим JSON envelope; добавки D-96 (std argv, serde, JCS-крейт, `File::lock`) — готовые библиотеки и укладываются в прежний диапазон (сверено 2026-10-21); best — готовые fixtures и предыдущие задачи переиспользуются; expected включает работу раздела 3, собственные проверки и передачу результата; неопределённость worst — отсоединённый запуск drain и обработка сигналов на обеих ОС. Это инженерный диапазон, не измеренная скорость.

**Уточнение объёма 2026-10-22:** пользователь перенёс предметную CLI-приёмку inspect/check в TK-12/TK-19; транспортные и process-проверки остаются здесь. Диапазон 4 / 6 / 10 ч не является новым измерением; ожидание готового TK-13 учитывается как внешняя зависимость, а не трудозатраты.

**Риск и действие:** Отсоединённый запуск drain на обеих ОС и интеграция готового TK-13; обработанная отмена остаётся TK-29. Зафиксировать воспроизводимый failing case и ограничить исправление указанной границей; если требуется смена принятого дизайна или работа выходит за worst — поставить blocked, вернуть решение на соответствующий gate и пересчитать план. Ожидание оборудования/весов не маскировать рабочими часами.

## 9. Отчёт о выполнении

**Текущий checkpoint Task 5, 2026-10-22.** Пользователь разрешил интеграцию готового
TK-13 `06dc8a18` в TK-11. Отдельный merge `f5611e2` сохраняет обе истории;
реальный drain подключён в `521276a`. [Полный отчёт](../../../../checks/TK-11/report.md#task-5-combined-production-drain--2026-10-22)
содержит разрешение конфликтов, принятые rulings, RED/GREEN, receipt hashes и границы.

Оба новых host/Linux ARM64 runner выполняют полный `./scripts/check-rust.sh`:
348 корневых тестов PASS, 0 FAIL, 4 ignored (3 запускаемых родителями helper entrypoints;
исторический 60-секундный provider deadline не повторялся). Отдельно проходят
12 сценариев настоящего product drain и runtime/lock observers. Все четыре итоговых
receipt inventory совпадают по 124 входам; последующая документационная правка
отражена отдельным overlay, исходные manifests не менялись.

Критерий реального drain подтверждают `cli_drain` в `task5-host-final` и
`task5-linux-final/container`: HTTP, вектор по hash, отсутствие работы, конкуренция,
исчерпание попыток, config/text races и смерть процессов. Критерий идемпотентности
подтверждает `direct_product_drain_accepts_eof_and_reuses_vectors_after_lost_response`
и сохранность cache revision/корпуса/Git index/refs. Обе финальные галочки остаются
открытыми до независимого Task 5 и итогового review ветки. Реальные inspect/check
остаются TK-12/TK-19; их unavailable-маршруты не запускают embeddings.

Следующие записи относятся к более ранним checkpoint и не меняют текущий статус.

**2026-10-22 — подготовка по Superpowers, реализация ещё не начата.** Созданы отдельные ветка `TK-11` и native worktree `/Users/zdkadiev/.codex/worktrees/tk-11/letopis` от `main`/`origin/main` на `12c5ad0c9f25e9a13f41a072fcac9570ff42ae75`. Базовый `./scripts/check-rust.sh` завершился с exit 0: fmt/compiler/Clippy и 172 Rust tests PASS, 0 failures; outer ignored child запускается родительскими process tests. Это проверка исходной базы, не приёмка ещё не созданного CLI.

[Утверждённая спецификация TK-11](../../../../../../superpowers/specs/2026-10-22-tk-11-cli-runtime-design.md) утверждена пользователем после независимой builder-проверки 2026-10-22. Пользователь принял разделение: TK-11 — CLI/JSON/отсоединённый запуск, TK-13 — config и embedding executor; реальный drain проверяется после их соединения, без второй реализации в TK-11. Независимая builder-проверка выявила пересечение приёмки `check`/`inspect`; пользователь утвердил уточнение карточек: TK-11 отвечает за CLI/runtime и контрактные проверки обработчиков, TK-12 — за реальный базовый inspect, TK-19 — за полный check. Критерии и ссылки трёх карточек согласованы; ни один критерий реализации не отмечен. Остальные границы TK-24/TK-29 и итоговый X-1 сохранены.

[План реализации TK-11](../../../../../../superpowers/plans/2026-10-22-tk-11-cli-runtime.md) подготовлен по Superpowers writing-plans: пять задач с интерфейсами, файлами, RED/GREEN и platform evidence. Пользователь утвердил план и выбрал Subagent-driven 2026-10-22. Реализация начата: Task 1 передана отдельному исполнителю; за каждой задачей следуют независимые spec/quality review, затем итоговое review ветки. Tasks 1–4 независимы от предметных read-алгоритмов; Task 5 подключения настоящего drain ждёт готового TK-13 в разрешённом интеграционном дереве. Критерии реализации пока не отмечены; фактические результаты будут внесены после проверки каждой задачи.

Evidence исходной проверки: `.artifacts/tk11/start/check-rust.log` в worktree TK-11, SHA-256 `d1d0af4382ddd010ed95bb401cbb502b0f7496774cd4163f494551b58ef1c909`. Исходный код, зависимости, исторические evidence, основной checkout и параллельный TK-13 не менялись.

После выполнения заполнить здесь: фактический результат и изменённые файлы; выполненные проверки с исходами; ссылки на evidence; оставшиеся блокеры/отклонения. `done` допустим только при выполненных критериях. При препятствии поставить `blocked` и указать причину, следующий шаг и затронутые зависимости.


**2026-10-22 — Task 4 реализована и проверена; TK-11 остаётся partial.**
[Отчёт и точные receipts](../../../../checks/TK-11/report.md): продуктовый executable,
композиция через текущий executable и application traits, один JSON при ошибках
ввода/сборки, отсутствие `.letopis` при malformed input. Полный root gate — 224 PASS,
0 failures, 2 child helpers, запускаемые родительскими тестами; rustdoc PASS;
7 Python runner tests PASS. macOS/Linux ARM64: совпали все 88 входных SHA-256;
на каждой платформе 31 CLI unit, 25 filesystem, 5 process и 3 executable tests PASS.
Два compiling controls отвергаются behavioral oracle и проходят после точного
восстановления исходников. Успех с test doubles отдельно обозначен; настоящий
бинарник возвращает unavailable для отсутствующих check/inspect/drain сервисов.

Критерий настоящего drain остаётся открытым до Task 5 с готовым TK-13. Сохранность
корпуса/index/refs после повторного и потерянного ответа проверена только для
недоступных маршрутов; критерий идемпотентности реального сервиса не закрыт.
Реальные inspect/check остаются TK-12/TK-19. Merge/push/GitHub не выполнялись.


**2026-10-22 — Tasks 1–4 прошли независимые spec/quality gates.** Для Task 4
нет Critical/Important; Minor о пути artifact при `CARGO_BUILD_TARGET` сохранён
в отчёте для финального ревью. Task 5 остановлена на утверждённом dependency gate:
TK-13 на `00bb62a1` ещё имеет открытые независимые Task 5/final reviews в своей
карточке и отчёте и не интегрирована сюда. Следующий шаг — получить завершённую
TK-13 через разрешённое объединение, подключить существующий executor и выполнить
реальную drain-приёмку и финальное ревью ветки. До этого статус `blocked`,
критерии реального drain/сервиса открыты; результаты Tasks 1–4 сохранены.


**2026-10-23 — Task5 fix round1 проверена; независимая приёмка открыта.**
Технический коммит `c81f5285344fe67b31eb2f17c932fc4f69974a91`. Полный immutable
baseline1034d01:2061 исходная мутация,550tool-caught/81missed/6timeout/1424unviable;
классификации сохранены. Все81misses разрешены62caught+19equivalent;19withheld
observer-only cases→17caught+2equivalent;6timeouts→4caught+2equivalent. Финальный
scope259:102caught/6missed/0timeout/151unviable; два meaningful miss затем пойманы
точным replay2/0/0/0. Финальный inventory2063,2040eligible,23узких equivalent
exclusions; все183local.rs accounted:179executed+4equivalent. Это отдельные scopes,
не новый полный прогон. Все11решений с ценой, точные proofs и source correspondence
сохранены в [плане](../../../../../../superpowers/plans/2026-10-22-tk-11-cli-runtime.md#task-5-fix-round-1--2026-10-23).

Host/Linux storage/native и CLI PASS: make verify V=1,390root/0failed/4ignored на
каждый debug/release profile,8emitted nested+2additional captured supervisor children
отдельно;12real-drain
scenarios на платформу. Setsid failure interposition подтверждён обеими платформами
и профилями. Storage runners отдельно выполнили root production deadline test:
1PASS,60.01s host/63.55s Linux; historical standalone manifests не запускались.
Storage242host/132container — точное общее подмножество; CLI131 равны полностью.
Dirty1034 host capture точно сопоставлен с committed242input hashes c81f528.
Исходные receipts сохранены; новые находятся в task5-fix1. Независимые fix/whole-
branch reviews и итоговый ledger/Engram остаются controller-owned. Inspect/check —
TK-12/TK-19. Merge/push/GitHub и изменения других worktrees не выполнялись.


**2026-10-23 — единая финальная правка M1 проверена; scoped re-review открыт.**
Negative control kernel-exclusion привязан к текущему retry_lock closure и обходит
только реальный kernel call, сохраняя использование helper при warnings=deny.
Новый лёгкий preflight проверяет точное единственное совпадение каждого anchor.
Python12/12 и оба реальных isolated compile→RED→exact restore→GREEN controls PASS
(macOSARM64), receipt `.artifacts/tk11/final-fix/controls/manifest.json`.
Из prior243 inputs изменены только2Python tooling paths; остальные241, включая все
Rust/build/native/schema inputs, совпадают побайтно. Старые runtime receipts/maps
сохранены; новые hashes и focused proof записаны в plan/report/final-fix-report.
390-test/native/deadline/mutation suites повторно не запускались. Все11решений и
23equivalents сохранены. Итоговая приёмка и ledger/Engram остаются root-owned;
main merge/push/GitHub и изменения других worktrees не выполнялись.

## Final acceptance — 2026-10-23

TK-11 is complete and accepted on branch `TK-11` at reviewed implementation
`62f2356143a485852ecb392f35d51c901e7f7536`; runtime Rust/build source is
`c81f5285344fe67b31eb2f17c932fc4f69974a91`. Completed TK-13 was integrated by
authorized merge `f5611e253baa6a506b41f059f66dfcd6e597354d`; no TK-11 merge into
main, push, or GitHub issue update is claimed.

All five plan tasks passed their task reviews. The full branch review
`12c5ad0..32c9914` found no Critical/Important issue and one Minor stale control
anchor. Its single fix wave `32c9914..62f2356` passed the independent scoped
re-review: M1 addressed, no new breakage or out-of-scope observations. All eleven
recorded rulings and their costs remain accepted; no new waiver was introduced.
The twelve explicitly declined broad-review subjects retain their previously
approved roadmap/contract boundaries, including actual check/inspect semantics
in TK-12/TK-19 and handled cancellation in TK-29.

Accepted verification combines the unchanged runtime inputs with the focused
Python correction; it does not relabel historical raw manifests as fresh runs:

- Host and Linux ARM64 storage/native and CLI drivers ran `make verify V=1`: each
  debug and release profile reports 390 passed, zero failed, four ignored entries.
  Eight emitted nested results per profile and two captured supervisor children
  remain separate. Dedicated real-product drain coverage passes 12 cases per platform.
- Native SQLite/vec1, standalone linkage and actual provider-deadline cases passed
  in their retained platform receipts. Evidence covers macOS/Linux glibc ARM64,
  controlled providers and process death; other platforms, live-model quality and
  physical power loss are not claimed.
- The original complete 2061-candidate mutation union remains
  **550 caught / 81 missed / 6 timeout / 1424 unviable**. Final affected 259-candidate
  execution remains **102 / 6 / 0 / 151**, followed by exact two-target replay
  **2 / 0 / 0 / 0**. Distinct affected resolution is 104 caught, four equivalent,
  151 unviable; all original gaps resolve through catches or the 23 individually
  justified narrow equivalents. This is composite evidence, not one final 2063 run.
- Only two Python tooling files changed after the full runtime receipts; all other
  241 of the prior 243 inputs are identical, including every Rust/build/native/schema
  input. New focused evidence has 12/12 Python passes and both actual compiling
  controls: intended RED 101, exact restoration, same-case GREEN 0. This focused
  execution is macOS ARM64 only. Old input maps and source captures remain intact.

The final-fix correspondence remains at
`.artifacts/tk11/final-fix/focused-validation-and-correspondence.json`
(SHA-256 `7e30f54c8d29a13739488d0172ad39648927b74fab05cb10ef69215a3d54585a`).
Final review/ledger/package records are retained in the verified plan-only archive
`.artifacts/tk11/sdd-final-2026-10-23/`; persistent task documents preserve decisions,
verification, equivalences and limits. Review report SHA-256 values:

- `task-5-rereview-1.md`: `0f269739a5d0e3a906c2d7f4f124efcae1420cb85f837be15552c092f91e6435`.
- `final-review.md`: `93d762d85808681fc267a4774f23a4470d0c264359ebef3a660f7abbcb166624`.
- `final-rereview.md`: `542a06aff71df52fbf098c104629b9dd4ab3ee74320ba7e99fa71f816f7e0547`.
