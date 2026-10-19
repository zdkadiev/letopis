# letopis — resilience intake и фиксированный R1

Дата: 2026-10-19. Навыки zkit:flow/zkit:resilience. Продолжение прямо запрошено пользователем после согласованной mechanics. Resource-math/contracts сохраняют записанные draft-ограничения; новый этап их не объявляет runtime-verified. Resource model, measurements, inference, production-код, Git commits и внешние сообщения не выполнялись/не менялись.

## Состояние и проверки

Согласованы concept, requirements, data, architecture, mechanics; ux неприменим по D-64. Flow-counter по agreed+skip: Step 7/10. Resilience отсутствовала, создана как draft. Исходные linker и checks requirements/resource-math/contracts/data/architecture/mechanics прошли. WARN 95% Must остаётся прежним и объяснённым. Протокол — resilience-intake-checks-2026-10-19.json.

Первичный register пересекает все 34 targets (6 B,12 I,4 X,12 E) с failure/threat. Получилось 9 F,5 T,8 O,8 DR. Это каталог дизайна; drills не запускались. Resilience checker правильно завершается ошибкой из-за отсутствия resilience_model.py; модель ожидает решения scope/RPO/RTO. Нулевые количества failing limits при 0 limits не являются passes/fits, clarity gate resilience не пройдена.

## Независимый gap-scan

Свежий read-only reviewer resilience_round_audit подтвердил T=3 без дополнительных продуктовых вопросов:

- G1 — product responsibility за backup/restore: внешний оператор/tool либо встроенный scope. Data §4, architecture Deferred и mechanics §9 явно откладывали полную потерю БД/диска. Git push/история не подменяют независимую копию БД и private snapshot refs.
- G2 — допустимая необратимая потеря при permanent medium/DB loss, включая source identity/time/config/errors, а не только Markdown edits. Это отдельная область от NFR-1 для power loss при работающих durable primitives. Можно явно принять отсутствие гарантированного backup window; такую цену не прятать в предполагаемый interval.
- G3 — hard restore-time target либо отсутствие жёсткой цели, когда исправная машина и пригодная копия уже доступны. Готовность = безопасные graph/FTS и долговечные данные, не обязательная регенерация всех vectors. Задержка обнаружения/действия оператора/подготовки машины считается отдельно.

Рекомендация автора для G1 — внешний backup/restore в v1 с описанным coherent copy/restore procedure, без новой команды и дополнительного сервиса. Это пока предложение, не принятое D-n. Фиксируем R1 T=3, закрыто0/3, очередь0. Ответ, уже разрешающий последующую развилку, записывается без церемониального повторного вопроса; новые gaps не расширяют текущий T.

## Исправления intake по аудиту

1. O-7 больше не выдаёт неопределённую restore-команду за существующий B-1/B-2 API. Владение copy evidence условно до G1; при внешнем scope это оператор/tool, letopis только проверяет своё состояние при обычном старте.
2. DR-2 дополнен queued/failed/attempt/seq, reindex state и recovery до обслуживания; восстановление копии не обнуляет бюджеты. Это существующие гарантии, не G4.
3. F-9 классифицирован H для несовместимой platform/build/storage, нарушающей Must. Один неверный wall clock отдельно M; новая временная гарантия не изобретена.

Не переоткрываются: D-76 protected phase,D-77 snapshots/live reads,D-78 max_retries,D-79 reserve semantics; local provider/CLI/status/no ACL/indefinite refs/no vector-drain deadline. Provider I/O deadline, corruption checks, secret redaction, copy coherence и детали drills — инженерные assumptions с именованной проверкой. Новые availability цифры, backup-сервис, telemetry/push или redundancy по шаблону не добавлены.

## Дальше

Задать только [resilience R1 Q1/3] G1. После ответов заполнить RA/RL и executable arithmetic model, затем final independent review. Этот gap-scan не заменяет stage closeout; resilience остаётся draft.

## Ответ G1 — 2026-10-19

Пользователь выбрал внешние инструменты/оператора для backup и restore, D-80. Встроенные команды/сервис не добавляются, документация coherent copy/restore остаётся обязательной частью дизайна. O-7 теперь имеет явного внешнего владельца, B-1/B-2 лишь existing startup/recovery validation. Закрыто 1/3, G2/G3 открыты, очередь 0. Периодичность и loss/restore targets не предполагаются из ответа. Первоначальные 0/3 выше — история intake. Модель пока не создана; новых измерений нет.

## Ответ G2 — 2026-10-19

Пользователь выбрал отсутствие фиксированного loss window v1 после permanent storage failure и восстановление последней пригодной внешней копии, D-81. Явно зафиксирован риск потери изменений после recovery point и невозможности восстановления без пригодной копии; нормативный backup interval не добавлен. NFR-1/протокол прерванных правок сохраняются. DR-2 проверяет точку копии и durable данные, а не выдуманный 24h bound. Закрыто 2/3, G3 открыт, очередь 0. Модель и stage closeout ещё не завершены; новых измерений нет.

## Ответ G3 — 2026-10-19

Пользователь выбрал отсутствие фиксированного restore-time deadline v1, D-82. R1 закрыт 3/3, всего три пользовательских решения D-80…D-82, очередь 0. Долговечное состояние проверяется, recovery завершается, graph/FTS пригодны к работе; все embeddings не ожидаются. D-83 — отдельные author-assumed technical details/what-if fixtures, не дополнительные пользовательские ответы. После интервью завершаются model/checks/review без физических drills и новых ресурсных измерений.
