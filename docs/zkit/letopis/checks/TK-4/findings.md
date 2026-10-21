# TK-4 — выводы для дизайна и передачи реализации

Дата: 2026-10-21. Основание: [MC-5](../../mechanics.md#mc-5--процессы-блокировки-и-отмена), A-AR1 в [architecture](../../architecture.md), D-94/D-95. Это результаты узкого runtime harness, использующего реальные неизменённые TK-2/TK-3, и уточнение доказанных границ принятого дизайна. Нового продуктового решения нет.

Локальная задача выполнена в ветке `TK-4` от `main@c8f414b6e0f739015728b62cccbecbfc54deafd8`. Доказательства ниже получены до интеграции. Затем пользователь запросил commit и локальное слияние в `main`; хеши результата и финальные проверки фиксируются в Git и общем Engram. [Issue #4](https://github.com/zaurkadiev/letopis/issues/4) содержит отчёт и остаётся открытой; push и закрытие issue требуют отдельного действия. G-0 остаётся открытым до TK-6.

## Подтверждённые свойства

| Вывод | Наблюдение и проверка | Следствие для реализации |
|---|---|---|
| Публикация сериализуется на `index.lock` | Десять процессов обнаруживают одну дельту; после повторной проверки под lock получается ровно одна новая ревизия и точные nodes/edges/files/FTS. Корпус не меняется. | Переиспользовать владение lock внутри TK-2; не брать второй внешний index lock вокруг publish. |
| Успешный probe не означает владение drain | Десять живых callers удержаны после освобождения успешных probes, затем допущены к spawn. Только один child получает `embed.lock`, девять отклоняются. | Caller освобождает probe до spawn; каждый drain самостоятельно берёт nonblocking lock и держит его весь цикл. Допустимы несколько кандидатов, активный drain один. |
| PID и наличие файла не определяют владение | После SIGKILL владельца lock доступен, хотя файл и старый PID остаются; живой посторонний PID также не блокирует новый запуск. | Не удалять lock-файлы и не заменять kernel flock проверкой PID. Права каталога 0700, файлов 0600; PID писать после acquisition и использовать только для диагностики. |
| Drain отделяется от caller | Проверены отдельная session, `/dev/null` для fd 0/1/2, завершение caller без ожидания drain и завершение drain после гибели caller. | `setsid` и закрытие/перенаправление всех трёх стандартных потоков входят в TK-11. |
| Потеря запуска оставляет восстанавливаемую работу | Missing executable, SIGKILL после probe и после подтверждённого spawn; следующий вызов без новой дельты находит и завершает оставшуюся работу. При отсутствии работы spawn отсутствует. | Не вводить очередь заданий или реестр процессов; работа выводится из текущих текстов без вектора. Ошибку spawn передать предупреждением в будущем CLI. |
| Граница отмены — commit | Восемь SIGINT cuts до commit: publish/reindex × projections/FTS/fingerprints/before_commit. Два cuts после commit сохраняют полное новое состояние. Проверяются реальный SIGINT exit status, точный кэш и неизменный корпус. | До commit незавершённая unit откатывается; после commit отмена не возвращает старую публикацию. |
| Отмена waiter не убивает владельцев | Для publish и reindex SIGINT отправлен после независимого подтверждения blocking flock. Publisher непрерывно держит lock; publisher и отдельный drain выживают и заканчивают работу. | Отмена относится к своему CLI-процессу; сигнал группе или чужому PID нарушает D-95. |
| Финальное состояние проверяется по данным | После завершения владельцев совпадают точные проекции, ревизия и native vector bytes, eligible work пуст. | Счётчика процессов или события «готово» недостаточно для acceptance. Использовать независимые state/kernel/vector oracles. |

## Все замечания независимого ревью

Один read-only reviewer проверил весь change и исходные receipts; самостоятельно платформенные команды не запускал. Critical — 0, Important — 2, Minor — 1. Исторический verdict «With fixes» относится к исходному snapshot. Оба Important исправлены за один проход; Minor закрыт последующим запросом пользователя. Второго независимого ревью нет. Полная история и RED/GREEN: [review.md](review.md).

**Пояснение слова «issues».** Речь о трёх замечаниях к тестовому стенду: риск послать сигнал другому процессу после повторного использования PID; риск оставить изменённые исходники и работающую вложенную команду при прерывании mutation tests; отсутствие доказательства, что отменяемый waiter уже вошёл в blocking flock. Все три устранены и проверены на macOS/Linux. Отдельных GitHub issues для них не создавали: результаты внесены в существующую issue #4 (TK-4). Дополнительно исправлены две устаревшие формулировки дизайна об отмене чужого процесса; это отдельная сверка документов, а не ещё два замечания reviewer.

| Замечание | Причина | Исправление и доказательство | Итог |
|---|---|---|---|
| Important: сигнал adopted child по numeric PID | После 20-секундного timeout child мог завершиться, быть reaped и уступить PID другому процессу; Event/socket сам по себе не закреплял process identity. Предварительная проверка liveness не убирала гонку. | Harness просит peer по его соединению выполнить self-SIGKILL; Event PID не является целью сигнала. Отключённый/поддельный Event не трогает живой decoy; direct child подтверждает настоящий self-SIGKILL exit status. | Исправлено; RED → GREEN. Это fixture control, не новый продуктовый IPC. |
| Important: прерывание mutation runner портило исходники и оставляло вложенную команду | Мутации меняли untracked worktree, `finally` обходился внешним SIGKILL, а новый session вложенного Cargo выводил его из группы очистки. | Только временные копии всех четырёх crates и отдельный build output; вложенный Cargo наследует группу owned runner. Тест прерывает настоящий runner при активной мутации: исходные SHA не меняются, nested socket получает EOF, группа общая. | Исправлено; RED → GREEN. Прерывание возвращает ненулевой exit. |
| Minor: waiter ещё не находился в blocking flock | Старый fixture hook стоял перед syscall, поэтому доказывал лишь отмену contender, наблюдавшего busy lock. | Новый тест сначала отвергает pre-flock hook, затем наблюдает syscall средствами ОС и только тогда посылает SIGINT. Седьмой compiling negative control удерживает hook и обязан упасть на этом же oracle. | Закрыто на macOS/Linux для publish и reindex. |

Дополнительная находка автора перед интеграцией: `unlink-held-file` действительно нарушал exclusion, но runner отвергал правильное падение `second active drain`, поскольку ожидал только `assertion failed: held`. Отключение защиты допускает оба исхода в зависимости от порядка процессов. Теперь только этот контроль принимает два точных признака нарушения и записывает фактически сработавший; compile failure, успешный тест или посторонняя ошибка по-прежнему отвергаются. [RED и исправление](report.md#pre-integration-oracle-follow-up) сохранены; обе платформы прошли полный runner заново для текущих SHA. Это четвёртая находка за всю работу, но не дополнительное замечание независимого reviewer.

## Платформы и сила доказательства

Финальные полные runs: macOS 26.6.2 ARM64 и Linux 7.0.0-22-generic aarch64/glibc 2.36 в Docker с `--init`. На каждой ОС: **10 Rust integration tests, 1 Python interruption test, 7 compiling negative controls**, fmt, Clippy `-D warnings`, полные TK-3/TK-2/TK-1 regressions и roadmap check — PASS. Все **60** текущих входных SHA256 совпадают на обеих платформах. На каждой сохранено **9** raw native observer artifacts с SHA256.

macOS observer — counted native `flock` frame из libsystem_kernel в `/usr/bin/sample`. Linux observer — `SYS_flock`, `LOCK_EX` и совпадение device/inode fd с удерживаемым `index.lock` через `/proc/PID/syscall`; captured wchan — `locks_lock_inode_wait`. На ARM64 номер SYS_flock — 32. Перед syscall hook наблюдатели видят ожидание socket и отвергают его. Наличие observer обязательно; timing-only fallback и skip отсутствуют. Observation bound 5 секунд, child socket/exit bounds 20 секунд; это ограничения стенда, не product SLO.

Семь независимых компилируемых отключений защиты обязаны дать invariant failure: убрать drain lock; считать PID владельцем; unlink удерживаемого файла; послать сигнал группе; наследовать session; наследовать stdout; оставить waiter до flock. Все компилируются с exit 0, падают на ожидаемой проверке с exit 101 и восстанавливают SHA исходной временной копии.

Стек: Rust 1.95.0, rusqlite 0.40.2, libsqlite3-sys 0.38.2 / SQLite 3.53.2, vec1 0.8. Точные pins, команды, manifest и logs: [report](report.md), [macOS](macos-arm64/manifest.json), [Linux](linux-arm64/manifest.json), [final verification](final-verification.json), [native observations](in-flock-verification.json). `before-review/`, `before-flock-followup/` и `before-unlink-oracle-followup/` — исторические receipts, не доказательство текущего кода.

Воспроизведение из корня worktree: `python3 -B tests/runtime/run.py --evidence NEW_DIRECTORY`. Linux command с image/cache/toolchain flags приведён в [README стенда](../../../../../tests/runtime/README.md).

## Все ограничения и последующая работа

| Граница | Принятое ограничение и последствие | Владелец следующей проверки |
|---|---|---|
| Независимое выполнение | Reviewer читал receipts и SHA; второго исполнителя и второго review pass не было. | Историю ревью сохранять без расширения claims. |
| Legacy и third-party audit | Helpers/pins TK-1…TK-3 не менялись; полные regressions PASS. Все upstream/legacy дефекты этим не исключены. | Аудит за пределами этой задачи не заявлен. |
| Production CLI, transport, cancellation, status и deployment | Стенд не поставляет готовый CLI, JSON warnings/status или реальный provider client. | TK-11, TK-29, TK-42. |
| Общая concurrency/liveness | Проверены выбранные schedules, а не fairness/stress, все interleavings или concurrent orphan reclamation. Работа между последним empty query и exit drain может ждать следующего вызова. Немедленный повторный запуск в этом окне не доказан. | Сохранить ограничение при переносе в TK-11; cancellation/system scenarios — TK-29/TK-42. |
| Другие OS/архитектуры/FS | Доказательства ограничены двумя измеренными ARM64 окружениями и их mounts. Windows, x86-64, иные/network FS не проверены. NFS уже вне поддержки. | Расширение платформенной матрицы требует отдельного измерения. |
| Физическая durability и ресурсы | Проверки process failure не заменяют power loss, ENOSPC/OOM, capacity, stat 10 000 файлов, RAM или throughput. Численных product SLO из fixture bounds нет. | Оставшиеся resource gates, DR-1/DR-7 и TK-42. |
| Входы, provider и локальный противник | Bounded trusted Markdown/config, fake vectors и fixture channels. Полный parser, arbitrary live in-place edits, general JCS, real transport/credentials и hostile local channel client не доказаны. | Задачи parser/provider и соответствующие system drills сохраняют scope. |
| Reaping adopted child | Kernel release/connection observable; настоящий self-SIGKILL status проверен direct child. Для grandchild нет собственного waitpid; reaping полагается на init. | TK-11 должен сохранять корректный lifecycle; Docker runs используют `--init`. |
| Интеграция | Исходный запрос отчёта не включал commit/merge/push. Последующим запросом разрешены commit и локальное слияние в main; отчёт в GitHub опубликован отдельно. | Push и закрытие issue остаются отдельными действиями. |

## Сверка проектных документов

При переносе результатов обнаружены две оставшиеся формулировки старого дизайна: A-AR1 предлагал проверять PID reuse при `--cancel`, T-2 говорил об отмене держателя чужого lock. Они приведены к уже принятому D-95: SIGINT собственного процесса, чужой PID только диагностический. Это исправление согласованности документации, не новое замечание reviewer и не добавление команды отмены.

В [MC-5](../../mechanics.md#mc-5--процессы-блокировки-и-отмена) закреплены результаты и handoff; [A-AR1](../../architecture.md) отделяет проверенную координацию от неизмеренного stat/resource бюджета; [resilience](../../resilience.md) связывает bounded process checks с остающимися DR-1/DR-6/DR-8. Исходные требования, владельцы компонентов, topology и gate criteria не меняются.
