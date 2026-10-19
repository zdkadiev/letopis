# R-3: Компактные Ollama embeddings — какие числа доступны для ресурсного расчёта?

> **Date:** 2026-10-18 · **Asked by:** resource-math, исходные параметры модели · **Feeds decision:** расчётный ориентир и проверка P-2 · **Status:** fresh

## Question
Какие характеристики 2–3 компактных моделей, доступных через Ollama, подтверждены первичными источниками? Можно ли по ним обосновать суммарный бюджет 1 ГБ для Engram и локального вычисления embeddings и нагрузку до 10 search/с? Область исследования ограничена по [D-40](../decisions.md#D-40).

## Findings
Размеры ниже — байты слоя модели из официального registry, не потребление ОЗУ и не полный размер установки. Векторы и контекст сверены с GGUF-метаданными и параметрами конкретных тегов.

| Кандидат | Слой модели, байт | Квантование | Размерность | Контекст: параметры / GGUF | Confidence |
|---|---:|---|---:|---|---|
| [all-minilm:22m](https://ollama.com/library/all-minilm:22m) | 45 949 216 | F16 | 384 | 256 / 512 | high |
| [embeddinggemma:300m-qat-q4_0](https://ollama.com/library/embeddinggemma:300m-qat-q4_0) | 238 530 688 | Q4_0, часть тензоров другого типа | 768 | 2048 / 2048 | high |
| [nomic-embed-text:v1.5](https://ollama.com/library/nomic-embed-text:v1.5) | 274 290 656 | F16 | 768 | 8192 / 2048; эффективный предел требует проверки | high для метаданных |

Точные размеры: manifests [MiniLM](https://registry.ollama.ai/v2/library/all-minilm/manifests/22m), [Gemma Q4](https://registry.ollama.ai/v2/library/embeddinggemma/manifests/300m-qat-q4_0), [Nomic](https://registry.ollama.ai/v2/library/nomic-embed-text/manifests/v1.5). Размерности: GGUF [MiniLM](https://ollama.com/library/all-minilm:22m/blobs/797b70c4edf8), [Gemma](https://ollama.com/library/embeddinggemma:300m-qat-q4_0/blobs/98e0321ff447), [Nomic](https://ollama.com/library/nomic-embed-text:v1.5/blobs/970aa74c0a90). Нужен именованный тег: обычный embeddinggemma:latest на момент проверки — около 622 МБ, а не Q4.

| # | Finding | Source | Confidence |
|---|---|---|---|
| 1 | API принимает строку или массив. По умолчанию слишком длинный ввод обрезается; truncate:false возвращает ошибку. Есть dimensions, keep_alive и измерения длительности/числа токенов. | [POST /api/embed](https://docs.ollama.com/api/embed) | high |
| 2 | Для retrieval Nomic требует разные текстовые префиксы search_query/search_document. Gemma 300m описывает разные query/document-шаблоны. Это требования подготовки текста при одной модели, не обоснование нового поля purpose. | [Nomic card](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5), [Google 300m card](https://huggingface.co/google/embeddinggemma-300m) | high |
| 3 | dimensions сокращает результат после вычисления исходного embedding; сокращение хранимого вектора само по себе не доказывает меньшую память инференса. | [handler на проверенном commit](https://github.com/ollama/ollama/blob/36a6e994d2f6c4ab94b3555185154d069e5dba87/server/routes.go) | high |
| 4 | В проверенной версии scheduler загружает embedding-модели с numParallel=1; пакетные входы внутри embed обрабатываются отдельно. Общую FAQ-формулу для параллельных generative-контекстов нельзя использовать как готовую оценку памяти embeddings. Это не доказательство полной сериализации всех API-вызовов. | [scheduler](https://github.com/ollama/ollama/blob/36a6e994d2f6c4ab94b3555185154d069e5dba87/server/sched.go), [FAQ](https://docs.ollama.com/faq) | high, зависит от версии |
| 5 | /api/ps показывает загруженные модели, size/size_vram/context_length; этих полей недостаточно для суммарного RSS Engram, Ollama, runner и рабочих буферов. | [API ps](https://docs.ollama.com/api/ps) | high |

**Конфликты источников:** сводная библиотека у MiniLM показывает 512, тег задаёт 256; maintainer описывает усечение после 256 wordpieces. У Nomic параметры задают 8192, GGUF — 2048, maintainer описывает дополнительное масштабирование контекста. Нужна проверка фактической версии Ollama. Новая [страница Google](https://ai.google.dev/gemma/docs/embeddinggemma) начинает с EmbeddingGemma 2; её 8K-контекст не переносится на исследованный 300m-тег.

**Не найдено:** сопоставимого первичного CPU-замера через Ollama на целевом ноутбуке с 4 ГБ, подтверждающего 1 ГБ суммарной памяти и требуемую смешанную нагрузку. Поиск ограничен официальными Ollama docs/source и карточками разработчиков моделей. Рекламный замер Google на EdgeTPU не является замером Ollama CPU. Запуски моделей не проводились. 8 КБ UTF-8 не задают число токенов; вместимость целых фрагментов и качество на корпусе проекта не подтверждены.

**Проверено основным агентом:** три tag/GGUF-страницы, registry manifests, embed/ps/FAQ, карточки моделей и embedding-ветвь scheduler на указанном commit. Поведение длинных входов и память запуском не проверялись.

## What it means for us
Это факты для расчёта, не выбор модели продукта. Если считать хранение одного float32-вектора на каждый из 100 000 элементов, полезные данные векторов составят N × d × 4: 153,6 МБ при 384 и 307,2 МБ при 768 измерениях. Это условный размер полезных данных без накладных расходов SQLite/vec1, а не требование загрузить все векторы в ОЗУ и не подтверждение бюджета.

Вердикт «помещается в 1 ГБ» пока не обоснован. Для P-2 нужны измерения суммарной памяти, холодных/прогретых запросов и смешанного потока поиска/индексирования на явно указанном оборудовании, с версией/tag/digest, длиной входов и параметрами batch/context. Сокращение размерности и автоматическое дробление текста не принимаются этим исследованием.

→ contracts/mechanics: определить обработку превышения контекста и подготовку query/document-текста без молчаливой потери содержимого; существующий порт texts → vectors остаётся исходной моделью.
→ roadmap: включить измерение P-2 и проверку качества на реальных фрагментах; отсутствие опубликованного замера не заменять выдуманной скоростью.

## Method
Два независимых read-only подагента: характеристики трёх моделей; поведение runtime и доступные измерения. Основной агент сверил ключевые факты с первичными источниками 2026-10-18. Ollama source pinned: 36a6e994d2f6c4ab94b3555185154d069e5dba87. Модели не устанавливались, установки/запуски/бенчмарки не выполнялись; другие провайдеры исключены по запросу пользователя.

## Используется в
<!-- собирается link_docs.py, не править вручную -->
- [requirements.md](../requirements.md): Changes since previous version, 7. Load profile and assumptions, 8. Журнал вопросов, Sources
- [resource-math.md](../resource-math.md): 5. Model, 8. Risks, assumptions and open questions, 9. Consistency check, Sources
- [data.md](../data.md): Sources
- [roadmap.md](../roadmap.md): 10. Согласование с исходными документами
