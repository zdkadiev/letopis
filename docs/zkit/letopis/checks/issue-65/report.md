# #65 — диагностика производительности Letopis

2026-10-30. Статус: model-free диагностика завершена; независимое ревью выполнено,
пять существенных замечаний исправлены и проверены регрессиями.
Оптимизации продукта не реализованы. Issue/gates не закрывались.

## Сборка и условия

Release Rust 1.95.0, locked/offline, `CARGO_PROFILE_RELEASE_DEBUG=1`.
Product commit: `e223dc689204d342e233cb5a21460e903eced204`; продуктовые исходники
совпадают с начальным `f7e0c6c90a7e4d4aa49e0a0c788c89ef7317c43b`.
SHA-256 отдельной неизменяемой копии бинарника:
`a132acb0a19c24440e8e97b881657c7b7adcc5c17052469f5dbc9d1e1b336f62`.
[Build receipt](../../../../../.artifacts/issue-65/build-02/build.json).

Хост: Apple M4 Pro, ARM64, 14 логических CPU, 48 GiB RAM, macOS 26.6.2;
`/usr/bin/git` 2.50.1 (Apple Git-155), Python 3.12.14. На старте 100k доступно
около 36 GiB диска; модель накопителя не измерялась. Хост не изолирован:
load average в manifest, отдельные smoke-профили пересекались с разработкой/сборкой.
Результаты — локальные диагностические наблюдения, не SLO и не статистически
доказанное ускорение. Основные кампании выполнялись последовательно; небольшие
проверки runner могли выполняться во время долгих вызовов.

Корпуса синтетические: seed 65, 2,000 UTF-8 bytes на фрагмент, 30 элементов на
файл по умолчанию, один source и пять рёбер на знание. Для 1k это 34 Markdown
файла и 5,000 рёбер; 10k — 334/50,000; 100k — 3,334/500,000. Source-файлы
шардированы по 100 строк. Они не являются production trace или usefulness-пилотом.
Каждый вызов — новый CLI-процесс. Embeddings выключены, кроме отдельной заглушки.
OS page cache не контролировался; обозначений «cold» нет.

## Основные наблюдения

[1k survey](../../../../../.artifacts/issue-65/survey-1k-03/summary.json):

| Сценарий | Успешных измерений | Медиана, с | Ограничение |
| --- | ---: | ---: | --- |
| Первая публикация + search | 1 | 202.193 | Отдельный first-use/full-audit лимит 600 с |
| Search, уникальный FTS-запрос | 20 | 0.817 | Только FTS, initialized projection |
| Inspect одного знания | 3 | 4.651 | Дорогие вызовы после conditioning |
| Traverse depth=2, limit=20, edge_limit=40 | 3 | 2.171 | Фиксированный ограниченный обход |
| Изменение одного файла + search | 3 | 8.007 | Каждый образец начинается с отдельного исходного состояния |
| Изменение двух файлов + search | 3 | 13.216 | Не повторный поиск после первой правки |
| Check | 0 | — | Один conditioning-вызов остановлен через 120 с |
| Status | 0 | — | Один conditioning-вызов остановлен через 120 с |
| Массовая правка + search | 0 | — | Остановлен через 120 с |
| Reindex | 0 | — | Остановлен через 120 с |

Таймауты сохранены как censored outcomes и не входят в успешные quantiles.
p95/p99 не публикуются: недостаточно наблюдений. Первый survey 1k с лимитом
первой публикации 120 с также остановлен; результат сохранён отдельно.

На [10k](../../../../../.artifacts/issue-65/survey-10k-01/completion.json) и
[100k](../../../../../.artifacts/issue-65/scale-100k-01/completion.json) первая
публикация не завершилась за 120 с. Последующие команды на этих масштабах
**not assessed**: initialized projection отсутствует. Экстраполяция успешных
1k-вызовов на них не делалась. Полные target-scale latency/coverage не доказаны.

У остановленного 100k вызова wait4 зафиксировал 106.794 CPU seconds и
123,961,344 bytes peak RSS (~118.2 MiB). Scope — direct child и учтённые ОС
reaped descendants, не сумма одновременной памяти дерева, не модель/whole stack
и не итоговый peak завершённой загрузки 100k. Для раннего 10k timeout ресурсы
ещё не сохранялись; это явный пробел, не нулевое потребление. После исправления
runner timeout-ресурсы сохраняются, регрессия проверена.

## Подтверждённые bottleneck и влияние

### 1. Повторные Git-проверки происхождения для каждого знания

В [контроле external-ссылок](../../../../../.artifacts/issue-65/source-control-60-01/comparison.json)
менялось только наличие объявлений source: строки ссылок заменены plain spaces
той же длины. Сохранены 60 знаний, 2,000 bytes на фрагмент, source bytes/tree,
packing, author source_snapshot, запрос check и бинарник. Source/rёбра закономерно
меняются как результат отсутствия external declarations.

| 60 знаний | Медиана check, с (n=3) | Git вызовов в отдельном counter-run |
| --- | ---: | ---: |
| С external-ссылками | 11.918 | 1183 |
| Без external declarations | 2.950 | 283 |

Разница ~8.97 с и 900 вызовов подтверждает существенную стоимость source-пути;
остаток ~2.95 с нельзя приписать ему целиком. Удалять ссылки или проверку
целостности в продукте не предлагается: контроль изменяет требуемую работу,
а не доказывает корректную оптимизацию.

Владельцы: [BasisPreflight](../../../../../src/service/document_refresher/basis.rs),
[ProvenanceService](../../../../../src/service/provenance.rs),
[ObjectDb](../../../../../src/git/objects.rs). `observe_sources` повторяет проверку
основания, чтение и хеширование адресованных historical/current bytes;
`read_blob` и `verify_object` запускают отдельные Git-проверки для дерева/объектов.
Этот путь вызывается при публикации, диагностике и проверке кандидатов поиска.

[30-секундный samply window 100k](../../../../../.artifacts/issue-65/window-100k-01/window/receipt.json)
с отдельным фильтром по PID Letopis подтверждает участие `observe_sources`,
`read_snapshot_hash`, `read_blob`, `verify_object`; Markdown parsing тоже виден.
CLI остановлен после 30.007 с, это **profile-only censored run**, не успешный search.
Wait4 CPU ~27.01 с имеет иной scope, чем ~4.97 с sampled CPU deltas выбранного
PID; их нельзя вычитать для получения «точного Git CPU» или смешивать с latency.
Ранжирование по CPU не равно процентам блокировки по wall-time.

### 2. Inspect захватывает корпус и повторяет per-blob Git verification

Запрос одного и того же первого знания при 60 элементах:

| Контроль packing | Markdown файлов | Медиана inspect, с (n=3) | Git calls |
| --- | ---: | ---: | ---: |
| 30 элементов/файл | 2 | 0.760 | 74 |
| 10 элементов/файл | 6 | 1.096 | 106 |

[Evidence](../../../../../.artifacts/issue-65/packing-control-60-01/comparison.json).
Не меняются node/source/edge totals, суммарные fragment bytes, source tree,
запрос и точные bytes первого адресованного фрагмента. Производные path IDs и
includes на границах файлов могут различаться; это зафиксированное ограничение
контроля packing. Разница 32 Git calls для четырёх дополнительных файлов
согласуется с просмотренным per-file capture/verification кодом, а не только
с шумом latency.

Владельцы: [InspectionService](../../../../../src/service/inspection.rs),
[ObjectDb::create_tree_from_blobs](../../../../../src/git/objects.rs).
Inspector читает/парсит все кандидаты, захватывает marked files и источники,
собирает промежуточное и итоговое дерево. Каждый blob проверяется отдельными
командами. Полный snapshot/проверки нельзя молча заменить неполным snapshot.

На 1k отдельный instrumented counter-run даёт 378 Git calls для inspect и
180 для traverse. Counter-run содержит overhead Python wrapper, его wall time
не используется как обычная latency. Git `GIT_*` environment очищается продуктом,
поэтому счётчики получены отдельным PATH-wrapper с неизменёнными argv/stdin/exit.

### Влияние длинного index.lock

[Controlled reindex overlap, 60](../../../../../.artifacts/issue-65/contention-60-01/reindex-overlap/receipt.json):
одинаковый reader занимает 0.685 с без writer и 12.704 с при reindex 12.584 с.
Перед запуском reader наблюдалась занятость index.lock именно в изолированном
репозитории с одним собственным writer. Оба результата валидны. Это подтверждённый
head-of-line эффект длинной application lock, а не доказательство SQLite lock
wait и не ошибка целостности. Ускорение source preflight должно проверяться
также по времени удержания этой блокировки; менять freshness-контракт нельзя.

## Профили и overhead

Samply 0.13.1, rate 1000 Hz, `--save-only --unstable-presymbolicate`.
[1k paired profiles](../../../../../.artifacts/issue-65/profile-1k-01/summary.json),
по 3 unprofiled и 3 profiled вызова с чередованием порядка и отдельными копиями:

| Команда | Unprofiled median, с | Profiled median, с | Наблюдаемый wrapper overhead |
| --- | ---: | ---: | ---: |
| Inspect | 4.183 | 4.648 | ~11.1% |
| Traverse | 1.909 | 2.339 | ~22.5% |

Профили содержат 1244–2670 usable stack samples, `.syms.json` и `stacks.json`.
Inclusive CPU attribution разных функций перекрывается; его нельзя суммировать.
Системный Apple Git не представлен как отдельный sampled process в этих
профилях; его затраты не выдаются за точный CPU-разбор самого CLI.
[Supplementary macOS sample](../../../../../.artifacts/issue-65/wall-check-60-01/wall/sample.txt)
снят с точного PID собственного check: видна цепочка observe_sources →
read_snapshot_hash → verify_object → wait4. Результат check валиден; эти
наблюдения не используются для точных процентов ожидания SQL/flock.

Для цензурированного профиля root wrapper завершает свой принадлежащий ему child,
чтобы samply сохранил профиль. Это отдельная серия с отдельным child outcome;
успех wrapper не означает успех CLI. Причина: в
[macOS lifecycle samply 0.13.1](https://github.com/mstange/samply/blob/samply-v0.13.1/samply/src/mac/profiler.rs#L182-L215)
профиль сохраняется после завершения root task; ограничение sampling duration
само по себе не заменяет watchdog дочернего приложения.

## Embedding client и concurrency

[Fake-provider assessment, 60](../../../../../.artifacts/issue-65/contention-60-01/completion.json):
60/60 current vectors, dimension 3, native read-only observer проверил config/hash
coverage; missing=0. Отдельный двухузловой canary: FTS для `vectorprobe` пуст,
через настоящую vector ветку возвращается `b.md`. Наличие только ok:true не
использовалось как доказательство vector participation.

Последовательный принадлежащий runner drain завершился за 1.406 с. Reader
успешно работал при paused fake inference; overlap ~0.689 с. Это доказательство
совместной работы с drain и корректного освобождения ownership, а не измерение
естественной SQL write contention. Отмена после завершения reader и освобождение
embed.lock проверены отдельным real-CLI тестом. Отсоединённые непринадлежащие
процессы не запускались для измерений; автоматический drain подавлялся lease
во время смены config, затем запускался напрямую.

Fake query с одним результатом занял 12.345 с, provider request delta=1. Это
client/storage/eligibility overhead с синтетическими векторами, а не скорость
модели или настоящий семантический benchmark. Полная проверка кандидатов до
output limit использует тот же дорогой provenance путь.

Для 1k/10k/100k fake coverage/overlap не оценены; переносить 60-node результат
нельзя. Настоящий hybrid, real-model drain и model RSS не запускались. #49 в
измеренном исходном коде не реализована: document drain последовательный.

## Покрытие, ограничения и оставшиеся пункты

- 60: все основные CLI сценарии выполнены как smoke; первоначальный ошибочный
  traversal oracle исправлен RED→GREEN, валидный traverse дополнительно измерен
  на 1k. Historical failed validation не переписана.
- 1k: все перечисленные FTS/read/refresh/reindex сценарии попытались измерить;
  четыре timeout-сценария явно не имеют успешной latency. Первый use успешен
  только в отдельном 600-секундном full-audit окне.
- 10k/100k: генерация и первая публикация попытались выполниться; первая
  публикация censored на 120 с, остальные initialized сценарии not assessed.
- CPU/RSS/I/O: scope и способ в каждом record. Wait4 block operations сохранены;
  I/O bytes, раздельный simultaneous process-tree peak, SQL query/transaction
  counts, точные SQLite/flock wait times и точный объём product parsed/hashed
  bytes **unavailable**. Размеры входов не подменяют реально прочитанные bytes.
- Fake server работает в принадлежащих runner потоках: handler CPU измеряется
  отдельно, его RSS отдельно не атрибутируется. Linux, 4-GB hardware, реальные
  модели и production workload не оценивались.
- Новая instrumentation в `src/` не понадобилась: профили/контроли уже выявили
  существенные причины. KISS/DDD/hexagonal boundaries и runtime не менялись.
- Следующие оптимизации подготовлены как [локальные issue drafts](issue-drafts.md).
  Публикация, GitHub closure и merge/push этой ветки не выполняются автоматически.

## Проверки и воспроизводимость

См. [runner README](../../../../../tests/performance/README.md),
[спецификацию](../../../../superpowers/specs/2026-10-30-issue-65-performance-design.md),
[план](../../../../superpowers/plans/2026-10-30-issue-65-performance.md).
Полный suite перед независимым review: **45/45 PASS, без skips**, 13.332 с.
Команда: `PERF_LETOPIS_BINARY=<immutable release> PERF_OBSERVER_BINARY=<native observer> PERF_BASELINE_FILE=<build receipt> /opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py' -q`. Generator: 24/24 PASS с Python 3.12 в PATH. Native vec1 observer
скомпилирован locked/offline; отдельная регрессия проверила actual coverage и
отказ при неверных expected counts. Продуктовые Rust suites не объявляются
повторно пройденными: продуктовые исходники, schema/native/build inputs не менялись.

Проверены hand mutants: binary identity/reset isolation, session cleanup,
timeout classification, inclusion of censored timings, wrong search IDs,
zero-sample profile, stdin forwarding, vector coverage, writer cancellation,
control edge counts и inclusive CPU double counting. Файлы восстановлены.
Обнаруженные runner defects (mutable Cargo artifact, missing provenance,
timeout RSS, freshness flag validation, short stage budget) закреплены отдельными
регрессионными тестами. [Evidence index](evidence-index.json): 153 raw artifacts с SHA-256;
проверены 223 сохранённых файлов harness из manifests. Raw profiles/receipts
локальны и git-ignored; индекс сохраняет ссылки и проверяемые hashes.

Перед review повторно проверены исходные 140 indexed hashes; итоговый index содержит 153 artifacts, длины функций ≤60,
ссылки документации и `git diff --check`.

[Независимое итоговое ревью и авторский fix pass](review.md): Critical 0,
Important 5 исправлены, Minor 0. После исправлений **56/56 тестов PASS без skips**;
пять review guards mutation-checked, файлы восстановлены. Дополнительный
[независимый oracle replay](../../../../../.artifacts/issue-65/inspection-revalidation-01/revalidation.json)
проверил все 17 retained inspect timings. Численные результаты не изменены;
дорогие кампании после lifecycle/oracle fixes не повторялись. Настоящий samply
проверен на намеренном внешнем watchdog: child interrupted/-15, без живого workload.
Повторное независимое одобрение исправленного дерева не заявляется.
Два optimization drafts ещё не опубликованы.
Продуктовые Rust/native inputs совпадают с исходным main; новые инфраструктурные
слои, runtime и изменения composition root отсутствуют.
