# TK-7 Markdown Domain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Реализовать переиспользуемую библиотеку доменных типов и read-only Markdown parser X-4 с точными raw/search SHA-256 и воспроизводимыми fixtures.

**Architecture:** Один корневой library crate `letopis`, без workspace/CLI. Домен содержит проверенные значения и рёбра; конкретный адаптер разбирает Markdown/JSON и возвращает целиком валидный файл либо диагностику. Git/SQL/provider/процессы не участвуют.

**Tech Stack:** Rust `1.95.0`, edition `2024`; `pulldown-cmark =0.13.4` (default-features=false), `sha2 =0.10.9`, `serde =1.0.228` с derive, `serde_json =1.0.145`; Cargo.lock. SHA/serde версии повторяют существующие proof crates. Новая версия pulldown проверена по [документации 0.13.4](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/); установка/сборка ещё не выполнены.

**Spec:** [2026-10-21-tk-7-markdown-domain-design.md](../specs/2026-10-21-tk-7-markdown-domain-design.md), согласована пользователем сообщением «Go on» после её передачи. План одобрен; пользователь выбрал «Go native». Выполнение отслеживается в журнале Native.

## Global Constraints

- Rust 1.95.0, edition 2024. Markdown — `pulldown-cmark`, offset iterator и `ENABLE_HEADING_ATTRIBUTES`; SHA-256 — `sha2`; JSON — `serde`/`serde_json`. Точные версии закрепляются при реализации по D-96.
- Только актуальная разметка X-4; нет совместимости с отменённой грамматикой, UUID, полем kind или закрывающим `<!-- /letopis -->`.
- Нет сериализатора, редактора корпуса, CLI-команд, асинхронного runtime, фоновых потоков, ORM, реестра плагинов или новых конфигурационных полей.
- Не добавляются лимит размера элемента, усечение, чанкинг, схемы произвольных атрибутов или наследование типов.
- Raw hash — точные исходные байты; search_hash — UTF-8 единого временного поискового текста. Нельзя получать эталон теста тестируемым extractor.
- Полуоткрытые байтовые диапазоны; строки 1-based, включительные. LF/CRLF не нормализуются при расчёте raw hash.
- Работать в `/Users/zdkadiev/.codex/worktrees/tk-7/letopis`, ветка `TK-7`; исходный main `48b8ed8`. Другие worktrees и существующие proof crates не менять.
- Фикстуры проверяются на macOS и Linux; x86_64/реальный filesystem containment/готовность всего продукта этим не доказываются.
- Локальные коммиты по задачам; merge/push/закрытие issue только по запросу пользователя. Краткая запись Engram по значимому результату, без дублирования всех логов.

## Review Focus

1. Unicode до границы элемента и CRLF/отсутствующий последний newline: byte offsets, строки и raw hash остаются точными — Task 2, `unicode_crlf_and_eof_have_exact_spans`.
2. Маркер внутри inline/fenced/indented code или незакрытого fence: не создаёт элемент/ребро и остаётся поисковым примером — Tasks 2/3, `code_examples_do_not_declare_elements`, `code_examples_remain_searchable`.
3. Reference definition вне диапазона элемента, форматированная подпись и image с похожим title: поисковая подпись сохраняется, граф не расширяется — Tasks 2/3, `only_inline_links_declare_edges`, `reference_labels_use_whole_file_context`.
4. Валидный первый элемент и невалидный второй, включая self-loop после нормализации пути: ошибка всего файла без частичной проекции — Task 2, `file_errors_never_return_partial_elements`.
5. Соседние Text events, emphasis, мягкие переносы и соседние блоки: слова не склеиваются и не разбиваются по случайным границам событий — Task 3, `search_text_preserves_word_and_block_boundaries`.

## File Structure

- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.gitignore`: корневой library crate, pinned stack, исключить только `/target/`; существующие untracked `.agents/`/`.engram/` не добавлять в Git.
- `src/lib.rs`, `src/domain/mod.rs`, `src/adapters/mod.rs`: публичные модульные границы.
- `src/domain/identity.rs`: RepoPath, Anchor, KnowledgeId, SourceUri и их проверка/нормализация.
- `src/domain/value.rs`: ValueError, ByteSpan, LineRange, TreeOid, Sha256.
- `src/domain/graph.rs`: ElementMetadata/Element, EdgeType/EdgeTarget/Edge, ElementKind и derived-kind query.
- `src/adapters/markdown/mod.rs`: parse, ParsedFile/ParsedElement, Diagnostic/DiagnosticKind; координация, без I/O.
- `src/adapters/markdown/scan.rs`: offset events, настоящие заголовки/комментарии и диапазоны элементов.
- `src/adapters/markdown/metadata.rs`: строгий serde wire-тип и преобразование в доменные метаданные.
- `src/adapters/markdown/links.rs`: графовые inline links, typed target, дедупликация и self-loop.
- `src/adapters/markdown/search.rs`: единственное поисковое представление по событиям полного файла.
- `tests/domain_values.rs`, `tests/markdown_elements.rs`, `tests/markdown_links.rs`, `tests/markdown_search.rs`, `tests/support/mod.rs`: предметные/контрактные проверки и общий loader эталонов.
- `tests/fixtures/markdown/{whole.md,sections.md,code.md,crlf.md,unicode.md,no-final-newline.md,references.md,expected.json}`: точные входные байты и независимые эталоны. Отрицательные короткие примеры — таблицы в тестах.
- `tests/markdown/Dockerfile`: только переносимый test environment Rust 1.95.0; runtime без сети.
- `docs/zkit/letopis/checks/TK-7/{report.md,macos-arm64.log,linux-arm64.log,verification.json}`: команды/результаты/хеши/границы доказательства.

Имя модуля — ответственность, не обязательство завести пустой файл: создавать каждый файл с его первой работающей частью. Domain не импортирует adapter/pulldown/serde/SQLite/Git/HTTP. Публичные типы имеют rustdoc; поля проверенных newtypes закрыты.

## Shared Interfaces

Это точные имена для всех трёх задач; необходимые стандартные derive/getters не дублируются в шагах.

```rust
// domain: FromStr не обязателен; все parse возвращают ValueError
RepoPath::parse(raw: &str) -> Result<RepoPath, ValueError>
Anchor::parse(raw: &str) -> Result<Anchor, ValueError>
KnowledgeId::parse(raw: &str) -> Result<KnowledgeId, ValueError>
KnowledgeId::new(path: RepoPath, anchor: Option<Anchor>) -> Result<KnowledgeId, ValueError>
SourceUri::parse(raw: &str) -> Result<SourceUri, ValueError>
TreeOid::parse(raw: &str) -> Result<TreeOid, ValueError>
ByteSpan::new(start: usize, end: usize) -> Result<ByteSpan, ValueError>
LineRange::new(start: u64, end: u64) -> Result<LineRange, ValueError>
Sha256::digest(bytes: &[u8]) -> Sha256
// RepoPath/Anchor/KnowledgeId/SourceUri/TreeOid/EdgeType имеют as_str();
// Sha256::to_hex() -> String; ByteSpan::start()/end() -> usize;
// LineRange::start()/end() -> u64.
ElementMetadata { pub created_at: u64, pub source_snapshot: Option<TreeOid> }
Element { pub id: KnowledgeId, pub metadata: ElementMetadata,
          pub bytes: ByteSpan, pub lines: LineRange, pub hash: Sha256 }
EdgeType::parse(raw: &str) -> Result<EdgeType, ValueError>
EdgeTarget = Knowledge(KnowledgeId) | Source(SourceUri)
Edge::new(source: KnowledgeId, target: EdgeTarget, edge_type: EdgeType)
    -> Result<Edge, ValueError>
// Edge getter names: source(), target(), edge_type(); derives Eq/Ord.
ElementKind = Vertex | Metavertex
kind_for(id: &KnowledgeId, edges: &BTreeSet<Edge>) -> ElementKind

// adapter: конечный интерфейс после Task 3
parse(path: &RepoPath, bytes: &[u8]) -> Result<Option<ParsedFile>, Vec<Diagnostic>>
ParsedFile { pub elements: Vec<ParsedElement>, pub edges: BTreeSet<Edge> }
ParsedElement { pub element: Element, pub search_text: String, pub search_hash: Sha256 }
Diagnostic { pub kind: DiagnosticKind, pub bytes: Option<ByteSpan>, pub message: String }
```

`DiagnosticKind`: InvalidUtf8, InvalidMetadata, InvalidAnchor, DuplicateAnchor, OrphanMetadata, NestedElement, InvalidLinkTitle, InvalidTarget, SelfLoop. Это внутренние локальные причины, не новые CLI error codes. Не сортировать диагностики по enum ordinal: возвращать в порядке появления в исходнике. Допустима остановка на первой ошибке файла; частичный успех недопустим.

`created_at` и номера строк представлены u64: переполнение при разборе возвращает диагностируемую ошибку представления, не panic/усечение; новая ресурсная квота этим не вводится. Парсер принимает RepoPath, но отдельно проверяет суффикс `.md`/`.markdown` для знания. UTF-8/path ошибки не вызывают I/O. ByteSpan не допускает start>end; привязку к длине файла обеспечивает parser.

---

### Task 1: Проверенные доменные значения и связи

**Files:** Create root package/config/module files, `src/domain/{identity,value,graph}.rs`, `tests/domain_values.rs`. SHA-256/serde зависимости добавляются здесь; pulldown — в Task 2, когда он нужен. Root package `name="letopis"`, `version="0.0.0"`, `publish=false`, `rust-version="1.95.0"`, только `[lib]` по умолчанию.

**Interfaces:** Consumes canonical X-1 ID patterns, X-4, нормализацию source из spec. Produces все domain signatures Shared Interfaces; serde wire-types в домен не попадают.

- [x] **Step 1: Написать failing domain tests и минимальный Cargo manifest для их запуска.** `ids_and_sources_normalize_from_git_root`, `anchors_and_ranges_reject_invalid_values`, `sha256_uses_exact_bytes`, `edge_identity_and_kind_are_derived`:

```rust
assert_eq!(SourceUri::parse("./src/../src/a.rs#L010-L025").unwrap().as_str(), "src/a.rs#L10-L25");
assert_eq!(KnowledgeId::parse("./knowledge/a.md#wal").unwrap().as_str(), "knowledge/a.md#wal");
for bad in ["", "1start", "L01-L02", "has space"] { assert!(Anchor::parse(bad).is_err()); }
for bad in ["/tmp/a.rs", "../a.rs", "a/../../b.rs", "a.rs#L0-L2", "a.rs#L3-L2"] {
    assert!(SourceUri::parse(bad).is_err());
}
assert!(KnowledgeId::parse("src/a.rs").is_err());
assert!(KnowledgeId::parse("a.md#L1-L2").is_err());
assert!(TreeOid::parse(&"a".repeat(40)).is_ok());
assert!(TreeOid::parse(&"a".repeat(64)).is_ok());
assert!(TreeOid::parse(&"A".repeat(40)).is_err());
assert!(ByteSpan::new(2, 1).is_err());
assert!(LineRange::new(0, 1).is_err());
assert_eq!(Sha256::digest(b"abc").to_hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
assert_ne!(Sha256::digest(b"x\n"), Sha256::digest(b"x\r\n"));
```

Для Edge tests создать source `a.md`, target `b.md`, типы includes/depends_on. Две одинаковые тройки дают BTreeSet.len()==1, добавление другого типа даёт 2. Только исходящий includes делает a metavertex; b остаётся vertex. `Edge::new(a, Knowledge(a), depends_on)` возвращает ошибку. Попытка includes → Source или external → Knowledge отклоняется. Не читать существование b.

- [x] **Step 2: RED.** `cargo test --test domain_values` должен провалиться из-за отсутствующих предметных API; сохранить результат. Ошибка загрузки крейта/сети не является RED поведения. Разрешается добавить только type stubs для компиляции, затем получить assertion failure.
- [x] **Step 3: Реализовать signatures Shared Interfaces в указанных domain files.** Нормализация — стек компонентов `/`: убрать `.`, свернуть `..`, запретить выход за корень, абсолютный/пустой путь, `#` и U+0000…001f в пути; сохранить регистр/Unicode. URI percent-decoding не добавлять. Якоря/TYPE проверять ASCII-правилами X-4, без regex dependency. Нормализовать source range без ведущих нулей, проверить overflow/end>=start. Hash — sha2, hex без нового hex crate. Derive Eq/Ord по всем частям тройки, без ID/даты/active. Edge::new проверяет соответствие: external требует EdgeTarget::Source, остальные типы — EdgeTarget::Knowledge; self-loop сравнивается по нормализованному адресу, включая разные варианты target.
- [x] **Step 4: GREEN.** `cargo test --test domain_values` и `cargo fmt --check` → PASS. Дополнить тестами: `created_at=0` не меняется, `.markdown`, сохранение регистра, обычные виды рёбер/параллельные типы, запрещённый uppercase TYPE, большие/нечисловые диапазоны без panic.
- [x] **Step 5: Commit.** `git add -- Cargo.toml Cargo.lock rust-toolchain.toml .gitignore src/lib.rs src/domain tests/domain_values.rs`, затем `git commit -m "feat: add TK-7 domain identities and graph values"`.

### Task 2: Markdown-элементы, метаданные, диапазоны и рёбра

**Files:** Create `src/adapters/{mod.rs,markdown/{mod,scan,metadata,links}.rs}`, `tests/{markdown_elements,markdown_links}.rs`, `tests/support/mod.rs`, все перечисленные fixtures и `expected.json`; modify root Cargo manifest/lock и `src/lib.rs`.

**Interfaces:** Consumes domain API Task 1. Produces `parse` с той же signature, но в этом промежуточном коммите `ParsedFile.elements: Vec<Element>`; Task 3 заменяет элементы на ParsedElement. Никаких пустых search_text/фиктивных hash в промежуточном результате. Внутри адаптера `scan` даёт полные offset events, границы metadata и элементов; links использует тот же полный файл, а не заново разобранные slices.

- [x] **Step 1: Написать failing fixtures и tests.** В `whole.md` точные ASCII bytes `<!-- letopis: {"created_at":0} -->\nX\n`. Ожидаются один элемент `knowledge/a.md`, bytes `0..len`, lines `1..2`, created_at=0 и raw SHA-256 из Python hashlib. В `sections.md` разместить unmarked preamble, `## A {#a}` + metadata, обычный `### sub`, затем неразмеченный `## stop`, затем `## B {#b}` + metadata. A заканчивается перед `## stop`, B у EOF; preamble/stop не элементы. Fixtures exact offsets и hash вне parser рассчитать Python stdlib по вручную указанным диапазонам; записать значения в expected.json, не пересчитывать parser-ом в тесте.

```rust
let path = RepoPath::parse("knowledge/a.md").unwrap();
let parsed = parse(&path, include_bytes!("fixtures/markdown/whole.md")).unwrap().unwrap();
assert_eq!(parsed.elements.len(), 1);
assert_eq!(parsed.elements[0].id.as_str(), "knowledge/a.md");
assert_eq!(parsed.elements[0].metadata.created_at, 0);
assert_eq!(parsed.elements[0].bytes.start(), 0);
assert_eq!(parsed.elements[0].lines.start(), 1);
assert_eq!(parsed.elements[0].lines.end(), 2);
assert!(parse(&path, b"# plain\nno metadata\n").unwrap().is_none());
assert!(parse(&path, b"\xff").is_err());
```

Дополнительные named tests и точные assertions:
- `unicode_crlf_and_eof_have_exact_spans`: UTF-8 кириллица перед/в заголовке, LF/CRLF/no-final-newline; сравнить полный slice, оба диапазона и pinned raw hash с expected.json. CRLF fixture записывается bytes, без Git EOL conversion; при необходимости добавить узкую `.gitattributes` только для fixtures.
- `code_examples_do_not_declare_elements`: fenced/indented/inline code и незакрытый fence с похожими metadata/heading/link дают 0 ложных элементов/рёбер; обычный валидный раздел вне code остаётся. HTML-комментарий должен быть настоящим Markdown event, а не substring внутри Code/Text.
- `file_errors_never_return_partial_elements`: второй размеченный раздел с unknown field/bad JSON/missing created_at/fraction/negative/overflow/null snapshot/duplicate JSON key/bad OID/nesting/duplicate anchor/orphan metadata/normalized self-loop делает весь Result Err; никакого доступа к частичным elements.
- `anchors_are_unique_across_real_headings`: одинаковый anchor на размеченном и неразмеченном настоящем заголовках → DuplicateAnchor; такой же текст в code не конфликтует.
- `only_inline_links_declare_edges`: обычные/reference/collapsed/shortcut links, autolink и image не создают ребро; inline title `letopis:type=includes` создаёт одну тройку; повторы дедуплицированы, другой тип создаёт ещё одну.
- `invalid_graph_titles_and_targets_fail_file`: TYPE empty/uppercase/с хвостом, выход за корень, invalid anchor/range и self-loop → нужная DiagnosticKind. Не-letopis title — навигация. Валидная отсутствующая цель допускается.

- [x] **Step 2: RED.** `cargo test --test markdown_elements --test markdown_links` → failures отсутствующего/минимального parse или assertion failures, не инфраструктурная ошибка. Проверить, что ожидаемые input/hash/line values действительно литеральные эталоны.
- [x] **Step 3: Реализовать `parse` и adapter modules.** Добавить pinned pulldown без HTML renderer/default features. `Parser::new_ext(text, Options::ENABLE_HEADING_ATTRIBUTES).into_offset_iter()`; сначала выявить реальные Markdown события/границы и привязку служебных комментариев, затем валидировать всё, потом возвращать результат. Для metadata — private `#[serde(deny_unknown_fields)]` wire struct, обязательный created_at, типизированный OID; `source_snapshot:null` отклонить отдельным wire-представлением отсутствующего/присутствующего поля: обычный Option по умолчанию принимает null и не соответствует schema. Дубли JSON-полей также отклоняются. Не назначать default для created_at. Точно сопоставлять metadata-line raw spans/соседнюю строку: не принимать многострочный JSON или комментарий, отделённый пустой строкой. Сохранять исходные bytes, а не восстановленный Markdown. Не объявлять headings/links из Event::Code/Text/HTML литералов; графовые links только `Tag::Link { link_type: LinkType::Inline, .. }` в пределах элемента. Переиспользовать доменные constructors и convert ошибок в DiagnosticKind.
- [x] **Step 4: GREEN.** `cargo test --test domain_values --test markdown_elements --test markdown_links`, затем fmt/clippy → PASS. Добавить regression для metadata первой строки/file-element с размеченным вложенным разделом (Err), двух соседних элементов (Ok), обычных подзаголовков (Ok), комментария не в первой строке/не после anchored heading (Err).
- [x] **Step 5: Commit.** Добавить только root package/module changes, adapter и перечисленные fixtures/test files, затем `git commit -m "feat: parse X-4 Markdown elements and graph links"`.

### Task 3: Общий поисковый текст, search_hash и приёмка TK-7

**Files:** Create `src/adapters/markdown/search.rs`, `tests/markdown_search.rs`, `tests/markdown/Dockerfile`, `docs/zkit/letopis/checks/TK-7/` evidence; modify adapter mod, tests элементов, expected.json, spec approval state, TK-7 task report. Roadmap менять только после подтверждённого done; оценки/расписание не менять.

**Interfaces:** Consumes Task 2 full-file events/validated element boundaries. Produces конечные ParsedElement/ParsedFile Shared Interfaces; остальные публичные signatures без изменений. Внутренняя `search::render` использует эти события и диапазон элемента, возвращает String; никакого второго parser/конвейера для embeddings.

- [x] **Step 1: Написать failing search golden и paired tests.** Выбор автора для внутреннего текста: Text/Code добавляют своё декодированное содержимое без пробелов между соседними inline events; SoftBreak/HardBreak дают `\n`; конец heading/paragraph/code block/item даёт один разделительный `\n`, если его ещё нет; удалять только внешние разделительные newline в начале/конце, не пробелы/отступы кода. Служебные metadata events исключаются, code c литеральным letopis-комментарием сохраняется. Link destination/title и heading attrs не попадают в текст. Обычный HTML не исполняется: HTML events сохраняются как буквальный текст кроме служебного комментария; внешние ресурсы не загружаются. Image даёт только alt text; reference definitions не добавляются текстом.

Простой golden: heading `## WAL {#wal}`, metadata, `Use **safe** [SQLite](knowledge/db.md "letopis:type=depends_on").` → `WAL\nUse safe SQLite.`. Пара абзацев `one\n\ntwo` → `one\ntwo`; `hel**lo**` → `hello`, а не `hel lo`.

`metadata_and_link_changes_keep_search_hash`: поочерёдно заменить created_at/source_snapshot, destination, TYPE и anchor; raw hash меняется, search_text/hash остаются. `own_text_label_and_code_change_search_hash`: изменить собственное слово, подпись и код; hash меняется. `moving_path_preserves_text_hashes`: те же bytes в a.md/b.md дают разные IDs, одинаковые raw/search hashes. `code_examples_remain_searchable`: буквальная служебная разметка в code присутствует в search_text, не удалена глобальным regex. `reference_labels_use_whole_file_context`: `[Shown][r]` в элементе и definition `[r]: ...` вне его → `Shown` остаётся, ребра нет; raw hash элемента не захватывает definition. `search_text_preserves_word_and_block_boundaries` проверяет emphasis, Code, soft/hard breaks и соседние блоки с точными строками.

- [x] **Step 2: RED.** `cargo test --test markdown_search` → assertion/отсутствующий search API failure; записать RED. Fixtures search expected strings и SHA-256 получают независимый Python hashlib, не вызов renderer.
- [x] **Step 3: Реализовать search::render и конечные ParsedElement.** Перевести Vec<Element> → Vec<ParsedElement>, обновить обращения Task 2 tests на `.element`, не ослаблять assertions. У каждого ParsedElement единственный временный search_text и его Sha256; в domain Element нет raw/search String. Не копировать полный корпус и не делать fetch links. Выполнить описанный внутренний выбор separator/HTML/code rules, зафиксированный golden, без публичного нового формата.
- [x] **Step 4: GREEN и portability.** На macOS выполнить `cargo test --locked`, `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings`; ожидание — все PASS, точное число тестов записывается после фактического запуска. Существующие proof crates не являются членами root workspace и не запускаются автоматически.

Для Linux Dockerfile: Использовать ту же закреплённую базу, что и TK-6: `rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1`; установить rustfmt/clippy при подготовке image, скопировать Cargo.toml/Cargo.lock/rust-toolchain.toml/src/tests, выполнить `cargo fetch --locked` на этапе подготовки с сетью. Build: `docker build -f tests/markdown/Dockerfile -t letopis-tk7-check .`. Запуск: `docker run --rm --network none letopis-tk7-check sh -c 'cargo test --locked --offline && cargo fmt --all --check && cargo clippy --locked --offline --all-targets -- -D warnings'`. Ожидание — те же fixtures/tests PASS. Использовать `COPY` только нужных путей, не `.git`, `.engram` или credentials. Записать image ID, architecture и реальные версии; не называть image proof x86_64. Недоступность Docker/registry — явно непроверенный Linux, не PASS.

- [x] **Step 5: Проверить приёмку и evidence.** report.md сопоставляет три критерия карточки с named tests/командами и фактическими исходами. verification.json хранит платформы, версии, полный argv, SHA-256 Cargo.lock/source/test/fixture файлов и логов. Golden fixture oracle должен обнаруживать ошибочный raw диапазон/нормализацию EOL; code fixtures — ошибочное объявление code-маркеров; paired tests — включение metadata в search text. Не внедрять отдельную универсальную mutation/benchmark infrastructure. Если обнаружен баг, узкий RED→GREEN regression перед исправлением. Указать оставшиеся Git/SQL/graph-validation/source проверки у владельцев из spec.
- [x] **Step 6: Полное ревью и закрытие локальной реализации.** По выбранному методу — один свежий whole-branch reviewer в Native либо task reviews+final в Subagent-driven. Исправить подтверждённые findings с regression, повторить только затронутые проверки, затем финальный `cargo test --locked`. Убрать устаревший draft state spec, заполнить task report и evidence; статус done допустим после всех acceptance и обеих платформ. Обновить строку статуса в roadmap без смены оценок и выполнить `python3 docs/zkit/letopis/roadmap/schedule.py --check` (Python 3.11+). Обновление issue #7 — при публикации по дальнейшему запросу; отметить в локальном отчёте, что его историческое слово serializer заменено текущим parser-only scope.
- [x] **Step 7: Commit.** Добавить search/test/Dockerfile/docs и только необходимые acceptance state changes; `git diff --cached --check`, затем `git commit -m "feat: add deterministic Markdown search text and TK-7 evidence"`. Сохранить один краткий итог/commit в существующую задачу Engram. Main не переключать и push не выполнять без запроса.

## Self-review

- Spec coverage: типы/границы domain → Task 1; X-4/bytes/metadata/links/errors → Task 2; search invariants/оба OS/evidence → Task 3. Запись корпуса и integrations исключены явно.
- Interface consistency: после Task 2 элементы — Element; ровно одна запланированная миграция Task 3 на ParsedElement.element. Конечный parse signature неизменен; typed targets и hash общие.
- Review Focus: пять указанных классов имеют named tests в задачах-владельцах.
- Proportion: три самостоятельных deliverables, setup/docs входят в соответствующую задачу; нет задачи ради scaffold, нового subsystem или универсального harness.
- User approval: spec и план одобрены; пользователь выбрал Native. Выбор автора в плане — Native: три части тесно связаны едиными типами и потоком событий; один финальный независимый reviewer достаточен при focused TDD.
