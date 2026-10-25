# TK-18 — валидатор графа и текущая диагностика

**Статус:** approved пользователем 2026-10-25 («Да»). Предложенные ниже решения
приняты в составе спецификации; implementation plan готовится до продуктового кода.
**Ветка:** tk-18, новый managed worktree, base main@75be0deead9d1317b2fd0f833e2bbc83a526c9b5.
**Разрешено пользователем:** перейти к разработке TK-18, отложив пилот и длительные
измерения. Обычные unit/integration, fmt/Clippy/verification остаются частью разработки.
Это исключение порядка [D-103](../../zkit/letopis/decisions.md#D-103), не прохождение G-1.

## Источники и результат

[Карточка TK-18](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-18-graph-validation.md),
[MC-1/MC-7](../../zkit/letopis/mechanics.md),
[X-4](../../zkit/letopis/contracts/X-4-markdown.md),
[I-11](../../zkit/letopis/contracts/I-11-diagnostics.md),
[структура](../../zkit/letopis/architecture.md#program-structure).

Результат TK-18 — реальный Rust-компонент: чистые правила графа плюс общий сервис,
который получает текущие проблемы из переданного read view. Полноценный CLI check
остаётся TK-19, status — TK-28, traverse — TK-23. Их интеграционные критерии в TK-18
не помечаются выполненными по одному API-тесту и не подменяются test double.
Отчёт разделяет готовность компонента и будущую приёмку команд; gate G-2 не пройден.

## Исходное состояние main@75be0de до реализации

- KnowledgeId = нормализованный путь или путь#якорь; ElementKind уже выводится из includes.
- Parser уже проверяет duplicate/nested anchors, metadata и direct self-loop.
  Edge::new отклоняет self-loop, поэтому успешный typed Edge не содержит его.
- DocumentRefresher публикует валидные изменения по файлу; files.valid/error хранят
  текущую ошибку невалидного файла и сохраняют его прежнюю валидную проекцию.
- StorageRead уже умеет list_nodes/list_edges/list_files/get_revision в одном snapshot.
- В src/ отсутствуют GraphProblem/find_graph_problems и общий graph diagnostic service.
- CLI Check имеет интерфейс/handler, но production wiring пока отсутствует.
- Make check на новом worktree прошёл; root product Rust ещё не менялся.
- Не используются implementation patches из приватных пилотных копий TK-17.
  Исторические proof harness не считаются продуктовой реализацией.

## Поведение домена

Предлагаемый API в domain, deterministic и без I/O:

    find_graph_problems(nodes: &BTreeSet<KnowledgeId>,
                        edges: &BTreeSet<Edge>) -> Vec<GraphProblem>

GraphProblem:
- DanglingTarget { source: KnowledgeId, target: KnowledgeId }.
- IncludesCycle { members: Vec<KnowledgeId> }.

Правила:
1. Рассматриваются рёбра от существующего knowledge source. Knowledge target,
   которого нет среди nodes, даёт одну проблему source/target независимо от числа
   типов рёбер. Сами объявления/параллельные рёбра не изменяются.
2. External Source targets не являются dangling knowledge; отсутствие bytes
   внешнего source — другая ответственность TK-20/TK-21.
3. Циклы ищутся только по includes между существующими знаниями. Обычные relations
   могут быть циклическими. Одна проблема на strongly connected component из >=2 узлов;
   компоненты разделены, shared child в DAG не создаёт ошибку.
4. Members отсортированы; dangling findings идут первыми по source/target,
   затем components лексикографически. Входные collections неизменны.
5. Добавление/удаление узла или ребра меняет следующий результат без persisted flags.
   Kind по исходящим includes остаётся существующим derive_element_kind.
6. Direct loop не получает недостижимый GraphProblem-вариант: он виден через
   существующую ошибку parser/files, а malformed storage decoding не даёт clean result.

**Алгоритм — предложение оператора:** итеративный двухпроходный SCC (Kosaraju),
stdlib collections, без рекурсивного DFS и graph crate. Узлы не разворачиваются
повторно, память линейна по входному graph metadata; точная стоимость на целевом
графе не объявляется измеренной. Не вводить квоту/обрезание результата.

## Общий прикладной сервис

Предлагаемый интерфейс в service/mod.rs:

    trait GraphDiagnostics {
        fn find_problems<R: StorageRead>(
            &self, view: &mut R, paths: &[RepoPath],
        ) -> CommandResult<Vec<CommandProblem>>;
    }

Concrete struct CurrentGraphDiagnostics в service/graph_diagnostics.rs реализует trait.
Он stateless, Debug/Default; не зависит от SqliteStorage/Git/HTTP/CLI и не владеет
процессом/lock. Caller refreshes перед read и передаёт уже закреплённый view.
Сервис не открывает вторую транзакцию, не вызывает provider и ничего не публикует.
Generic dispatch предпочтительнее dyn; non-object-safe trait здесь осознан.

Сервис читает knowledge metadata, все declared edges и files из ОДНОГО view:
- invalid file даёт validation_failed с details.path и фиксированной безопасной
  причиной существующего parser category; arbitrary stored text не выводится.
  Предложение оператора: DiagnosticKind получает общий get_message/get_code;
  Markdown Diagnostic::new_at использует тот же get_message без изменения текстов.
  Exact mapping известного files.error -> DiagnosticKind выполняется в service:
  invalid_utf8/invalid_metadata/invalid_anchor/duplicate_anchor/orphan_metadata/
  nested_element/invalid_link_title/invalid_target/self_loop. Unknown/None даёт
  reason=invalid_markup и фиксированное сообщение invalid knowledge file, без
  включения stored payload. Ни schema, ни parser grammar не меняются;
- dangling даёт details.path владельца source, element_id=source, target и reason=dangling_target;
- cycle даёт finding на каждую затронутую пару (component, file) с path, reason=includes_cycle
  и одним локальным element_id, без копирования полного members в каждую строку.
  Representative = лексикографически минимальный нормализованный KnowledgeId
  данного файла внутри данной SCC (предложение оператора для детерминизма);
- при нескольких SCC одного файла findings не сливаются. Размер выдачи остаётся
  линейным по членам SCC; недопустима квадратичная копия массива members по файлам;
- empty paths = весь корпус, иначе фильтруются только владельцы problems.
  Graph всегда рассматривается целиком, включая cross-file зависимости;
- стабильный порядок по path/reason/element_id/target. Дубли selectors не дублируют findings.
  Отсутствующий в files выбранный путь сам по себе не новый not_found контракт;
  в TK-18 paths — фильтр владельцев, публичный CLI input/ошибки принадлежат TK-19;
- read/storage failure возвращает typed CommandError с приватной причиной;
  частичного успешного ответа вместо ошибки нет. IDs/paths не превращаются в raw source payload.

Для невалидного файла диагноз берётся из files текущей публикации. Дополнительные
graph problems относятся к сохранённой валидной проекции этого же read view,
а не к попытке заново разобрать broken live bytes. Bad file не подавляет findings других.

## Физические файлы и сборка

Предлагается добавить:
- src/domain/validation.rs — GraphProblem, чистый алгоритм, private helpers.
- src/service/graph_diagnostics.rs — одна concrete struct и её impl.
Изменить domain/mod.rs и service/mod.rs: явные declarations/reexports/interface.
service/model.rs получает shared DiagnosticKind message/code helpers; минимальная
замена message match в markdown/diagnostic.rs убирает дублирование, не меняя грамматику.
lib.rs получает wire_graph_diagnostics() -> impl GraphDiagnostics с реальным
CurrentGraphDiagnostics::new(), без I/O; caller-owned read остаётся у caller по I-11.
Это product library entry point для сборки будущих handlers, не второй read-service.

Не создавать ports/adapters/contracts каталоги, façade над каждым методом,
новую таблицу проблем, runtime/dependency, отдельную очередь или HTTP API.
Оставить существующие Markdown/Git/storage границы. Parser менять только при
воспроизведённом пропуске правила; already-correct поведение не переписывать.

## Проверки реализации и архитектуры

Обычные проверки нового Rust-кода, не отдельная roadmap-задача тестирования:
- dangling pair с несколькими relation types; цель появляется/исчезает без правки source;
- external/missing-source edges, legal ordinary cycles, disconnected includes SCC и DAG;
- длинная includes-цепь и обратный edge без рекурсии/stack overflow;
- snapshot-consistent file+graph report, выбор paths не обрезает входной graph;
- invalid markup/self-loop одного файла не скрывает проблему другого; исправление
  удаляет текущий file finding; стабильные IDs после вставки строк;
- public JSON-safe problems не содержат marker raw body/unknown persisted reason;
  storage error не превращается в пустой/частичный успех;
- реальный DocumentRefresher + SqliteStorage + lib factory/API, без изменения Markdown
  bytes и без persisted problems. Это компонентная интеграция, не CLI приёмка.

Говорящие тесты, Arrange/Act/Assert, независимые expectations и ручные targeted
mutation controls; fmt/check/Clippy/strict Rustdoc/release по make check/make verify.
Не запускать paid pilot, RAM/load/platform proof suites или whole-crate mutation sweep.
Изменения storage/native/schema сейчас не нужны; если обнаружится такая необходимость,
отразить её в плане и отдельно определить обязательный platform runner.

## Статус и передача

Спецификация утверждена. [Implementation plan](../plans/2026-10-25-tk-18-graph-validation.md)
утверждён пользователем и выполнен Subagent-driven. Компонент реализован; все task/final reviews Approved. См. [отчёт](../../zkit/letopis/checks/TK-18/report.md).
TK-19 использует этот компонент для настоящего check; TK-28 — для status;
TK-23 отдельно обеспечивает bounded traversal без отсутствующих endpoints и повторного
расширения узлов. Их интеграции явно отмечаются как pending до фактической реализации.
Блокеры TK-16/TK-17 сохраняются; начать разработку не значит подтвердить G-1.


## Builder check

Один fresh Luna/high reviewer прочитал предложение. Два уточнения закрыты как
операторские implementation choices: представитель SCC/file — минимум ID;
persisted file reason распознаётся только по известным DiagnosticKind сообщениям,
неизвестное содержимое не публикуется. Эти решения предложены здесь явно,
не приписаны пользователю. Остальных существенных вопросов к реализации не осталось.
