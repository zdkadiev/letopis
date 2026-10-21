# TK-7 — независимое whole-branch ревью

Fresh reviewer: gpt-6-astra/high, без истории основной сессии, read-only. Диапазон `48b8ed8e47df2bc6b5df18a4048f50a840b63d6c..79cc52df9737424dfbfab7b85e4e862b9f6b63f7`. Это единственное ревью кода в выбранном Native workflow.

## Отчёт reviewer до исправлений

Strengths: домен отделён от Markdown/JSON; нормализация и ограничения связей централизованы; общий разбор сохраняет reference context; raw hash — исходные байты; ошибки атомарны по файлу. Reviewer повторно запустил 27 тестов в копии под /tmp и проверил source/log SHA-256 из verification.json.

Critical: нет. Important: два.

1. `scan.rs:25`: отдельная Html-строка внутри `<script>` с текстом `<!-- letopis: {"created_at":2} -->` принималась за служебный комментарий. Валидный файл с первым file-element становился Err(OrphanMetadata). pulldown выдаёт Html по строкам, а проверка одного префикса теряет контекст блока. Ожидается один элемент и буквальный script-текст в search representation.
2. `links.rs:12`: `![see [target](b.md "letopis:type=includes")](img.png)` создавал includes и делал источник метавершиной. Link event вложен в Image, поэтому collector должен учитывать родительский контекст. Ожидаются подпись `see target` и отсутствие ребра; обычная ссылка вокруг изображения должна сохраниться.

Minor: нет. Исходный verdict: **With fixes**. Reviewer подтвердил оба случая компилируемыми падающими probes в своей /tmp-копии; production checkout не менял.

## Declined to judge и решения исполнителя

- CR-only: обещаны LF/CRLF, отдельной поддержки CR-only нет. Решение: не заявлять её; цена ошибки — старый CR-only файл может потребовать перевода EOL перед индексацией.
- Дополнительные heading attributes и контейнерные заголовки: используется поведение выбранного CommonMark parser с X-4 проверками anchor и standalone metadata-line. Решение: не вводить дополнительную ограничивающую грамматику; цена ошибки — уточнение X-4 может потребовать новых fixtures/ограничений.
- Существование targets, symlink containment, includes cycles и Git/SQL/CLI/model integration: явно следующие задачи. Решение: сохранить границу чистого parser; цена ошибки — нельзя использовать его успех как доказательство этих гарантий до integration tasks.
- Большие корпуса и x86_64: текущие проверки не устанавливают масштаб/платформу. Решение: сохранить численные требования, но не объявлять их выполненными; цена ошибки — ограничения/пересмотр после соответствующих измерений.
- Linux reviewer независимо не повторял: проверил receipt/hash. Решение: evidence исполнителя с полным offline Linux run достаточно для этой задачи; цена ошибки — независимого воспроизведения этой платформы нет.

## Исправления — один проход RED→GREEN

Оба finding подтверждены как Important по эффекту: потеря индексации валидного файла и ложный граф. Решения не меняют согласованный продуктовый scope.

| Finding | Regression | Исправление | Проверка |
|---|---|---|---|
| Raw HTML | raw_html_content_does_not_declare_metadata | Объявлять metadata по началу HtmlBlock/реальному InlineHtml; не по произвольной Html-строке script/style/pre/textarea/обычного комментария | RED → GREEN, 10 element tests PASS |
| Image alt link | image_alt_links_do_not_declare_edges | Collector отслеживает Image depth и игнорирует вложенные Link; ссылки вокруг Image сохраняются | RED → GREEN; links_wrapping_images_still_declare_edges PASS |

[RED/GREEN logs](tdd/). Полный macOS suite после обоих исправлений — **30/30 PASS**, fmt/clippy PASS. Linux evidence обновляется на исправленных исходниках перед закрытием. Повторное ревью не запускалось: Native предписывает один fix pass с regression и полным suite. Отложенных minor нет.
