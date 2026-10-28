# TK-30: чек-лист готовности (state: unready)

Состояние подготовки: **unready**. Ни одна платная сессия не выполнялась
(`model_executions = 0`). `python3.12 tests/final_pilot/run.py audit --root DIR` выводит
состояние только из явно заполненных значений manifest; отсутствие ошибок, успешный
`local_tools_ok` или локальный smoke не означают доступ к модели, прохождение gate или
готовность. Пока есть хоть один пункт ниже, `audit` возвращает `unready` и код 2.

| Pending-ключ | Что должно быть зафиксировано до freeze | Владелец |
| --- | --- | --- |
| `seeds` | seed commit и полный starting inventory каждой цепочки/условия | TK-30 Task 2 |
| `product` | финальная post-TK-29 base/сборка продукта (текущая `d260348` не финальная) | после TK-29 |
| `memory_versions` | версии read-only бинарника памяти и skill из интегрированного продукта | после TK-29 |
| `embedding` | embedding provider/model/config полного профиля памяти | оператор |
| `assets` | закреплённые assets и image (CLI `0.164.0`, toolchain, digest) | TK-30 Task 4/5 |
| `model` | model, model revision (или явно `unknown`), бюджет времени/ходов/токенов/retries | оператор |
| `agent_config` | SHA-256 общей конфигурации агента, одинаковой для обоих условий | оператор |
| `g2` | ссылка на evidence G-2 | оператор |
| `prompt_freeze` | **prompt wording**: человек читает все девять prompts и `instructions.md` и фиксирует формулировки; пока не сделано, prompts остаются черновиком | человек |
| `preflight` | evidence полного preflight: tools, установленный memory CLI, digest image и `git_target_separation` (локальный smoke их не доказывает) | оператор |
| `participant_isolation` | evidence изоляции participant (отдельные HOME/CODEX_HOME, нет доступа к operator/другому условию); команда запуска TK-31 обязана использовать **тот же mount builder**, что и Docker boundary proof, иначе proof тавтологичен | оператор |
| `correlation_attribute` | evidence подтверждённого атрибута корреляции request inventory ↔ usage counters в живой telemetry | оператор |
| `base_drift_recheck` | повторная проверка исходников выбранной base: непроходящий новый критерий каждой задачи на seed; запись действует только для точных `base_commit` и `selection_sha256` | оператор |

Значения pending-ключей считаются предоставленными, только если они непустые: `null`,
пустые строки/контейнеры, голые `true`/`false` и числа не очищают пункт. Gate (`g2`,
`prompt_freeze`, `preflight`, `participant_isolation`, `correlation_attribute`) очищается
лишь объектом с непустой ссылкой `evidence`; интерпретация gate evidence остаётся
ручным шагом человека, `audit` её не оценивает. Без этих трёх evidence-gates `audit` остаётся
`unready`, даже если все остальные значения заполнены.

`audit` пересчитывает hash `selection.json` и hash копий входов в `inputs/` и сверяет их с
`source_receipts`; расхождение, как и неполный `original_base`, даёт `input_drift`.

Любое изменение проверенного commit, `selection.json` или prompts делает прежнюю запись
`base_drift_recheck` недействительной: статус `ready` не переносится, проверка повторяется.

Нерешённые для этого этапа: фактический доступ к модели и login, живая telemetry,
качество embeddings и G-3/G-4 не проверялись и не заявляются. `selection.json`,
check IDs и recurring facts — operator-only; participant получает только prompt своей
задачи и `instructions.md`.
