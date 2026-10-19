# R-7: Совместимость embeddings Ollama и OpenAI

> **Date:** 2026-10-18 · **Asked by:** contracts, X-2 · **Feeds decision:** D-65 и выбор протокола Ollama · **Status:** fresh

## Question
Позволяет ли OpenAI-совместимый endpoint Ollama унифицировать embeddings, сохранив запрет молчаливого усечения длинного ввода?

## Findings
| Finding | Source | Confidence |
|---|---|---|
| Ollama документирует POST /v1/embeddings, model, input, encoding format, dimensions. Совместимость относится к подмножеству OpenAI API | [Ollama compatibility](https://docs.ollama.com/api/openai-compatibility) | high |
| OpenAI принимает model/input и encoding_format:float; ответ содержит data с embedding и index. Размерность зависит от модели; доступность dimensions не универсальна | [OpenAI embeddings](https://developers.openai.com/api/reference/resources/embeddings/methods/create) | high |
| Нативный Ollama /api/embed принимает truncate:false и возвращает ошибку при выходе за контекст; значение по умолчанию true | [Ollama embed](https://docs.ollama.com/api/embed) | high |
| Совместимая структура EmbedRequest не содержит truncate; middleware создаёт api.EmbedRequest только с Model, Input, Dimensions. Поэтому произвольное добавление truncate:false в совместимый JSON не обеспечивает запрет усечения | [request](https://github.com/ollama/ollama/blob/36a6e994d2f6c4ab94b3555185154d069e5dba87/openai/openai.go#L96), [conversion](https://github.com/ollama/ollama/blob/36a6e994d2f6c4ab94b3555185154d069e5dba87/middleware/openai.go#L398) | high, pinned revision |
| EmbedHandler при неустановленном Truncate идёт в ветку усечения; при false проверяет длину и возвращает HTTP 400 для превышения | [handler](https://github.com/ollama/ollama/blob/36a6e994d2f6c4ab94b3555185154d069e5dba87/server/routes.go#L1276) | high, pinned revision |

## What it means for us
Приняты обе интеграции по D-65. Общий порт и базовая HTTP-инфраструктура возможны; вывод «один совместимый endpoint полностью выполняет D-61» не подтверждён и для проверенного кода неверен. Рекомендация: нативный Ollama с truncate:false и OpenAI-совместимый протокол для остальных. Альтернатива — только совместимый протокол с отдельным механизмом гарантированного отказа до усечения. Первоначально это были варианты к обсуждению; после ответа пользователя вариант с нативным Ollama принят по [D-66](../decisions.md#D-66). Никакой новый токенизатор или параметр не вводится по результатам проверки.

## Method and limits
Прямая узкая проверка основным агентом: официальные документации OpenAI/Ollama, исходники и GitHub commits/main. HEAD Ollama на момент проверки: 36a6e994d2f6c4ab94b3555185154d069e5dba87, commit timestamp 2026-10-18T00:31:52Z. Отдельная делегация не потребовалась: вопрос закрыт документацией и прямым чтением преобразователя запросов. Источники рассматривались как данные. Модели, inference и нагрузочные эксперименты не запускались; ресурсные результаты не обновлялись. Это не проверка каждого стороннего OpenAI-совместимого сервера и не полная сертификация границы X-2. Будущая версия Ollama может изменить преобразователь; ссылки закреплены на проверенном коммите.

## Используется в
<!-- собирается link_docs.py, не править вручную -->
- [contracts.md](../contracts.md): Changes since previous version, X-2
- [architecture.md](../architecture.md): Sources
