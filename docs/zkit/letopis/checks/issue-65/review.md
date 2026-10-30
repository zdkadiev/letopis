# #65 — независимое ревью и исправления

2026-10-30. Один свежий reviewer (gpt-6-astra, high) проверил диапазон
`f7e0c6c..d01d7ae`, спецификацию, план, код, отчёт и raw evidence.
Реализация — Native; отдельные implementer subagents не использовались.

Reviewer самостоятельно подтвердил 45/45 исходных тестов, все 140 исходных
indexed hashes и численные таблицы отчёта. Critical: 0. Important: 5.
Minor: 0. Исходный verdict: **With fixes**. Ниже — один авторский fix pass с
RED→GREEN и полным suite; повторное независимое ревью не заявляется.

## Пять исправлений

1. **Outer timeout оставлял nested window workload живым.**
   Regression: `test_outer_timeout_stops_nested_group_and_pipe_holding_descendant`.
   Window теперь принимает TERM/INT как cancellation flag, не прерывая создание
   ещё не взятого во владение Popen. Collector завершает nested session с коротким
   cleanup grace; внешний collector даёт grace всей группе, даже если sampler
   leader уже умер. Проверены TERM-resistant child и descendant с унаследованными
   pipes, sampler-like proxy и пять последовательных отмен. Дополнительно
   настоящий samply прерван секундным stage watchdog: `window/child.json`
   сохранён с interrupted/-15; stage failure ожидаем.

2. **Ctrl-C проглатывался, запускались следующие сценарии.**
   `append_record` сначала flush/fsync сохраняет interrupted attempt, затем
   поднимает KeyboardInterrupt до controller. Ожидаемая scheduler cancellation
   writer остаётся отдельным schedule receipt. Добавлены stream regression,
   настоящий SIGINT controller, propagation для profile window/wall sampler и
   прерывание ожидания reindex writer. Последующие сценарии не запускаются;
   completion пользовательской отмены — interrupted.

3. **Writer мог умереть раньше reader и освободить embed.lock.**
   Supervisor наблюдает ранний exit как до inference barrier, так и во время
   reader; останавливает и reap reader перед собственной отменой writer.
   Barrier deadline согласован с invocation budget. `FixtureProcesses` владеет
   уникальной приватной копией неизменённого executable и учитывает surviving
   setsid successors по точному полному пути, с повторной проверкой PID identity
   перед сигналом. Это не поиск/kill по basename. Нет возможности inspect exact
   identities — запуск такой фикстуры отклоняется заранее.
   Проверены ранний writer exit против медленного reader, настоящий detached
   drain, повторно использованный PID и выживание unrelated sentinel.

4. **Inspect oracle принимал другой корректно хешированный фрагмент.**
   До вызова продукта фиксируются ожидаемые anchor/range/hash из canonical
   fixture bytes. Ответ обязан совпасть с этим независимым ожиданием, затем
   проверяются exact bytes из возвращённого Git snapshot. Wrong valid fragment
   и omitted range отвергаются. Новый oracle повторно проверил **17 сохранённых
   inspect timing records**, все валидны; исходные receipts не переписаны.

5. **Wall sampling refresh использовал уже опубликованную правку counter-run.**
   Для wall sampling подготавливается отдельная equivalent pre-edit fixture.
   Regression моделирует публикацию counter-run и отвергает повторное использование
   этого состояния. Retained inspect/traverse profiles этим дефектом не затронуты.

Все пять review guards дополнительно проверены targeted hand mutants: каждый
пойман, файлы восстановлены. [Mutation evidence](../../../../../.artifacts/issue-65/review-fix-mutations-01/results.json).
Полный suite после восстановления: **56/56 PASS без skips**, 21.745 с.
Новые tests/owner — только tooling; продуктовые Rust/native inputs не изменены.

## Что reviewer оставил вне оценки и решение автора

- Real-model/hybrid, usefulness и model RSS: остаются unassessed по согласованному
  model-free scope. Цена ограничения — нельзя делать выводы о модели/пользе памяти.
- Завершённые 10k/100k latency и completed-load RAM: не оценены из-за censored
  публикации. Цена — нет target-scale capacity verdict.
- Linux/4-GB, точные SQL/flock waits/counters: не оценены. Цена — нет platform/NFR
  verdict; scope метрик явно указан, нули не подставлены.
- Optimization speedups/gates: оптимизации не реализованы, gates не закрывались.
  Цена — improvement criteria в drafts остаются гипотезами.
- Исторические mutation runs: исходные записи остаются авторскими наблюдениями
  и ledger, а не утверждением о полном независимом replay. Все пять актуальных
  review guards заново mutation-checked с сохранением logs; дорогое повторение
  всех исторических экспериментов не выполнялось.

## Решения исполнения

В порядке принятия; ограничения/цена каждого решения указаны явно.

1. Python 3.12 вместо системного 3.9 — выполняет минимальную версию плана;
   без подходящего интерпретатора запуск не поддерживается.
2. Проверка JSON результата принадлежит orchestration/report, не binary-stream
   collector — controller обязан выполнить oracle до успешной статистики.
3. Копии проекции обновляются empty FTS search перед правкой, full audit измеряется
   отдельно — исходные bytes/метаданные и oracle должны оставаться валидны.
4. Read calls дольше двух секунд получают три measured observations — данных
   недостаточно для p95/p99; фактическое n показано.
5. Fake server работает в собственных потоках runner — отдельный server RSS
   недоступен, handler CPU имеет узкий scope.
6. Inference barrier проверяет overlap с drain — это не SQL lock-wait measurement.
7. First publication допускает отдельное окно 600 с — routine calls остаются
   на 120 с; first-use не смешивается с повторными вызовами.
8. Сохраняются исходники harness в evidence — дополнительный небольшой расход
   диска позволяет воспроизвести незакоммиченные версии.
9. Поздние profiles используют проверенные idle synthetic fixture copies —
   оригинальные evidence не меняются; metadata copies предварительно refresh.
10. 1k profiles выполнены до крупных surveys после неожиданно долгой публикации —
    выводы не экстраполируются на неоценённые масштабы.
11. Censored CPU window использует отдельный root wrapper — wrapper success не
    является CLI success; nested cancellation закреплена regression.
12. Review шага 6 — тот же единственный Native whole-branch review — после fix pass
    независимое повторное одобрение не заявляется.
