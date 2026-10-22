# TK-8 Git Snapshots Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Реализовать продуктовый Git-адаптер, который создаёт tree из исходных байтов, читает заданный снимок и удерживает его через служебный ref после GC.

**Architecture:** Один crate, consumer-owned GitSnapshotPort в service/ports и конкретная структура GitCliAdapter в adapters/git. Domain остаётся чистым; lib.rs явно создаёт доступный Git-компонент и возвращает его через интерфейс. Реализация синхронная, std::process argv/stdin; CLI/services/storage/provider появляются в своих TK.

**Tech Stack:** Rust 1.95.0, edition 2024; std и существующие domain-типы; внешний Git ≥ 2.29; macOS/Linux glibc ARM64; Docker с закреплённым Rust base. Новых runtime/dev dependencies не требуется.

**Spec:** [2026-10-22-tk-8-git-snapshots-design.md](../specs/2026-10-22-tk-8-git-snapshots-design.md). Пользователь после уточнения структуры/идиоматичности/CLI/utils и распространения правил на все задачи разрешил переход к следующему шагу сообщением «Идем по флоу /superpowers». Это основание для данного плана; план подтверждён выбором пользователя «Subagent»; все шаги выполнены, обе независимые review-находки исправлены и проверены.

## Global Constraints

- Ветка TK-8, worktree `/Users/zdkadiev/.codex/worktrees/tk-8/letopis`, base `main@2a7e8db0758e3410b5ec4327848b7c06185907e4`. Исходный root baseline — 30 integration tests PASS; после документации source не менялся.
- [Структура и её приёмка](../../zkit/letopis/architecture.md#program-structure), [TK-8](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-8-git-snapshots.md), X-3 и I-12 обязательны. Порт/адаптер/композиция находятся в реальных отдельных файлах, trait реализуется структурой через impl; зависимости к домену и контрактам.
- Сборка в lib.rs; main остаётся будущим тонким запуском CLI. Не создавать CLI handlers, будущие сервисы, Storage/Embedding или пустые модули. Domain и parser TK-7 не реорганизуются.
- Git ≥ 2.29, полный lowercase tree OID SHA-1/SHA-256 без обрезания; commit/tag не принимаются за snapshot. Исходные bytes без filters/textconv/EOL conversion; новые обычные файлы mode 100644.
- Корпус, пользовательские index/HEAD/refs/config не меняются. Разрешены Git objects и refs/letopis/snapshots/<oid>. Нет shell, libgit2/gix, Tokio, retries, фонового worker или нового production timeout. Короткая scoped std thread для передачи stdin допустима только внутри одного subprocess call, чтобы одновременно читать stdout/stderr.
- Один ref namespace, pin до выдачи inspect; нет деревьев публикаций, TTL, purge, auto commits/GC или SQL-копий текста. Неполное дерево/сбой pin не дают успешного RetainedTree.
- Per-command core.fsync=all/core.fsyncMethod=fsync; физическая durability, file/directory/full-flush барьеры и power-loss остаются неподтверждёнными до P-3/TK-38. Минимум Git 2.29 не объявляется реально запущенной версией без такого прогона.
- Изменять только собственные paths. Новые helpers — локальные, общие tests/support/git.rs только для повторного использования; приватность минимально необходимая. Публичные API с rustdoc, typed Result, fmt/Clippy -D warnings.

## Review Focus

Пять условий, которые нужно дополнительно закрепить тестами сверх прямой happy-path приёмки спецификации:

1. Partial/promisor repository с отсутствующим blob: чтение не получает его из remote, повреждение не маскируется как missing path. Test `promisor_miss_never_fetches` в цикле B/C.
2. Имена со специальным смыслом для Git pathspec (`*`, `:`, leading dash) остаются точными именами, а не шаблонами. Test `paths_are_literal_and_never_shell_input` в цикле B.
3. Ref уже существует как symbolic ref на пользовательскую ветку: pin отклоняется, ветка и symbolic ref сохранены. Test `symbolic_and_conflicting_refs_are_rejected` в цикле C.
4. Два процесса одновременно закрепляют один tree: оба успеха подтверждены одним точным direct ref, без перезаписи чужого значения. Test `concurrent_pin_is_idempotent` в цикле C.
5. Подпроцесс пишет большой stderr до чтения большого stdin либо рано закрывает stdin: нет взаимной блокировки, ребёнок reaped, публичная ошибка безопасна. Test `subprocess_drains_and_reaps_on_io_failure` в цикле A.

## Файлы и ответственность

- Create `src/service/mod.rs`, `src/service/ports/mod.rs`: только объявление реально используемого Git-порта.
- Create `src/service/ports/git.rs`: GitSnapshotPort, SnapshotFile, RetainedTree, SnapshotErrorKind/SnapshotError; контракт без Git execution.
- Create `src/adapters/git/mod.rs`: GitCliAdapter, open, impl порта и вызов приватных операций. Конкретный тип/конструктор доступны внутри crate; внешние consumers получают impl GitSnapshotPort из lib.rs.
- Create `src/adapters/git/process.rs`: запуск/IO/reaping, очищение Git-routing environment, безопасные ошибки, version gate. Только этот файл вызывает std::process::Command для продукта.
- Create `src/adapters/git/objects.rs`: проверка object format/type/existence, raw read с проверкой OID, точный обход ls-tree, построение blobs/trees.
- Create `src/adapters/git/refs.rs`: closure candidate и CAS pin в служебном namespace.
- Create `src/adapters/git/tests.rs`: unit/private-boundary tests с управляемой executable fixture; без публичных test hooks.
- Modify `src/adapters/mod.rs`, `src/lib.rs`: Git-модуль, service-модуль и явная сборка open_git_snapshots.
- Create `tests/support/git.rs`: std-only TempRepo/обёртка прямых Git fixture calls; SHA1/SHA256, staged/live снимки, checksum oracle. Используется integration tests и cfg(test) git/tests.rs через path module.
- Create `tests/git_snapshots.rs`, `tests/git_retention.rs`: публичный factory/trait contract и настоящие Git/GC/process scenarios.
- Create `tests/git/Dockerfile`, `tests/git/README.md`; modify `.dockerignore` только для этих тестов/fixtures. Старые retained harnesses/evidence не переписывать.
- Create `docs/zkit/letopis/checks/TK-8/{report.md,verification.json,review.md,execution-ledger.md}` и реальные logs/tdd receipts; update TK-8 card, EP-1 и roadmap только после приёмки. Spec/plan статусы обновляются по реальному прогрессу.

## Task 1: Законченный GitSnapshotPort и subprocess-адаптер

Создание, чтение и pin — три операции одного небольшого компонента и одной реализации порта. Они выполняются тремя циклами TDD внутри одной задачи, чтобы не выдавать частичный trait или пустой forwarding-сервис за самостоятельный результат. Промежуточные локальные commits допустимы после зелёного цикла; окончательный task-done только после всех операций и платформ.

**Files:** перечислены выше; дополнительных подсистем/пакетов не создавать.

**Interfaces — consumes:** существующие `RepoPath::as_str()`, `TreeOid::parse(&str) -> Result<TreeOid, ValueError>`, `TreeOid::as_str()`; std Path/bytes. Domain-типы не меняются.

**Interfaces — produces (`service/ports/git.rs`, `lib.rs`):**

```rust
pub struct SnapshotFile<'a> {
    pub path: &'a RepoPath,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotErrorKind { InvalidRequest, NotFound, InternalError }
// SnapshotError: private fields, Display + Error; methods specified below.
// Display/Debug expose only static operation context and safe status;
// underlying I/O/exit cause is preserved without raw stderr/stdin.
pub struct SnapshotError { /* private fields */ }

pub struct RetainedTree { /* private TreeOid */ }

pub trait GitSnapshotPort {
    fn create_tree(&self, files: &[SnapshotFile<'_>])
        -> Result<TreeOid, SnapshotError>;
    fn read_blob(&self, tree: &TreeOid, path: &RepoPath)
        -> Result<Vec<u8>, SnapshotError>;
    fn pin(&self, tree: &TreeOid)
        -> Result<RetainedTree, SnapshotError>;
}

pub fn open_git_snapshots(start: &std::path::Path)
    -> Result<impl GitSnapshotPort, SnapshotError>;
```

Публичные accessors: `SnapshotError::kind(&self) -> SnapshotErrorKind`, `SnapshotError::message(&self) -> &str`, `RetainedTree::oid(&self) -> &TreeOid`. Поля остаются приватными; safe Display/Error не раскрывают raw subprocess payload.

Test helper contract (`tests/support/git.rs`): `ObjectFormat::{Sha1,Sha256}`, `TempRepo::new(format: ObjectFormat) -> TempRepo`, `root(&self) -> &Path`, `subdir(&self) -> &Path`, `git(&self, args: &[&str], input: Option<&[u8]>) -> std::process::Output`, `head_index_config_and_live_snapshot(&self) -> FixtureState`, `change_and_delete_live_files(&self, path: &str)`, `direct_ref(&self, oid: &str) -> String`, `expire_reflogs_gc_prune_now_and_pack_refs(&self)`, `fsck_strict_no_reflogs(&self) -> std::process::ExitStatus`. FixtureState сравнимый: исходные HEAD/index/config, пользовательские refs и известные fixture live-файлы; objects/служебные snapshot refs не входят. Helper запускает только прямые Git argv в принадлежащих тесту каталогах, изолирует пользовательский config и очищает routing env. Fixture methods создают/assert успешный setup, никакая setup failure не считается успешным negative control. Std-only helper пригоден и для cfg(test) без импорта crate letopis.

`RetainedTree` конструируется внутри crate только после успешного pin; tests не создают его напрямую. Порт не определяет CLI DTO/HTTP/SQL types. Names above are fixed for this plan; private helper names below are equally fixed where referenced.

### Цикл A — обнаружение repository и безопасный запуск

- [x] **A1. Написать тесты в git/tests.rs и std fixture helper.** Проверяемые assertions (таблица задаёт обязательные случаи):

```rust
assert!(check_version(b"git version 2.29.0\n").is_ok());
assert!(check_version(b"git version 2.50.1 (Apple Git-155)\n").is_ok());
assert_eq!(check_version(b"git version 2.28.7\n").unwrap_err().kind(), InternalError);
assert_eq!(check_version(b"unknown\n").unwrap_err().kind(), InternalError);
// open_rejects_old_git_before_object_calls: spy has --version only; error mentions 2.29.
// discovers_root_from_subdirectory: operation from subdir addresses repo root.
// open_rejects_non_repository_and_bare: no arbitrary working directory accepted.
// subprocess_drains_and_reaps_on_io_failure: fake executable writes >=256 KiB
// stderr before reading >=1 MiB stdin, then exits nonzero; call terminates,
// owned child is reaped, stdin/stderr CANARY absent from Display/Debug.
```

Private tests may use `GitCliAdapter::open_with_process(start, GitProcess::with_executable(path.to_path_buf()))`. This seam is crate-private, not a new public option. Fake executable is generated in disposable test directory; no PATH/env mutation in the multi-threaded test process. Runtime test waits are bounded by an owned subprocess supervisor; production receives no new timeout.

- [x] **A2. Зафиксировать RED.** Run `cargo +1.95.0 test --locked --offline --lib adapters::git::tests -- --nocapture`; сначала тесты, затем только минимальные объявления API для компиляции. Не считать compiler/import error поведенческим RED; сохранить assertion failure на неподдержанной версии/пустом boundary в `checks/TK-8/tdd/`.
- [x] **A3. Реализовать процессную границу.** `process.rs`: `check_version(output: &[u8]) -> Result<(), SnapshotError>` и `GitProcess::run(&self, cwd: &Path, args: &[OsString], input: Option<&[u8]>) -> Result<Vec<u8>, SnapshotError>`. Одновременно обслуживать stdin/stdout/stderr и всегда закрывать stdin/reap ребёнка, включая BrokenPipe. `mod.rs`: `GitCliAdapter::open(start: &Path) -> Result<Self, SnapshotError>` и приватный `open_with_process(start: &Path, process: GitProcess) -> Result<Self, SnapshotError>`. `GitProcess::system() -> Self` выбирает системный git; `with_executable(program: PathBuf) -> Self` нужен private fixture. Порядок: --version/gate, rev-parse --show-toplevel, проверка рабочей копии, rev-parse --show-object-format (только sha1/sha256). Сохранить абсолютный root и object format; все следующие команды привязаны к root.
- [x] **A4. Зафиксировать environment/argv policy и GREEN.** Убирать наследуемые GIT_* overrides, затем явно ставить `GIT_NO_REPLACE_OBJECTS=1`, `GIT_ALLOW_PROTOCOL=""`, `GIT_NO_LAZY_FETCH=1`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`. Передавать `-c core.hooksPath=/dev/null`; write calls также core.fsync=all/core.fsyncMethod=fsync. Empty allow-list блокирует transport даже на Git 2.29; новый env no-lazy-fetch сам по себе старую версию не защищает. Private command-policy tests проверяют routing/config-env removal и stdout/stderr redaction. Повторить A2: PASS.

### Цикл B — точные bytes и деревья

- [x] **B1. Написать integration tests публичной factory/trait.** В tests/git_snapshots.rs каждый сценарий выполняется для SHA1 и SHA256, создаёт свежий TempRepo:

```rust
let git = open_git_snapshots(repo.subdir()).unwrap();
let s1 = git.create_tree(&files).unwrap();
assert_eq!(git.read_blob(&s1, &path).unwrap(), original_bytes);
assert_eq!(git.create_tree(&reordered_files).unwrap(), s1);
assert_eq!(repo.head_index_config_and_live_snapshot(), before);
repo.change_and_delete_live_files(path.as_str());
assert_eq!(git.read_blob(&s1, &path).unwrap(), original_bytes);
```

Имена/дополнительные assertions: `exact_bytes_without_index_or_commit` (nested/empty/binary/Unicode/LF/CRLF/no final newline, pre-existing staged/unstaged state, active clean/EOL filter canary never runs); `paths_are_literal_and_never_shell_input` (`dir/a:b`, `-name`, `literal*.md`, `$(touch SENTINEL).md`, actual sentinel absent); `empty_tree_and_invalid_paths` (empty tree; duplicate, a+a/b, .git/.letopis rejected before writes); `tree_types_and_missing_objects_are_distinct` (wrong OID format length for repo/commit/tag => InvalidRequest; absent root/path => NotFound; dangling child/decoded content with wrong OID => InternalError); `replacement_refs_do_not_change_bytes` (replace ref ignored). Error messages/assertions exclude canary data.

- [x] **B2. Зафиксировать RED.** Run `cargo +1.95.0 test --locked --offline --test git_snapshots -- --nocapture`. Добавлять только нужные compiling API stubs при первичном RED, не объявляя trait готовым до цикла C. Сохранить failing exact-byte/type/identity assertion, затем заменить stub реальным кодом.
- [x] **B3. Реализовать read/type/existence в objects.rs.** Приватные сигнатуры: `ObjectDb::validate_tree(&self, oid: &TreeOid) -> Result<(), SnapshotError>`, `ObjectDb::read_blob(&self, oid: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError>`. Полный OID сверяется с object format. Batch-check raw OID отличает missing root от ошибки Git; точный type tree проверяется без peeling. Для пути читать непосредственные записи ls-tree -z и сравнивать имена побайтово по компонентам, не передавать user path как Git pathspec/revision expression. Существующий leaf blob читается cat-file blob <blob_oid>; промежуточный non-tree/missing name => NotFound, leaf non-blob => InvalidRequest. Missing child object => InternalError. Raw object bytes проверять повторным `hash-object -t <type> --no-filters --stdin` **без -w** против запрошенного OID, чтобы обнаружить корректно распаковываемую порчу. Ни один путь не переключается на live file.
- [x] **B4. Реализовать создание в objects.rs.** `ObjectDb::create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError>`. До записи валидировать все paths/дубли/prefix collisions; .git components и root .letopis исключены. Затем hash-object -w --no-filters для каждого входного byte slice и bottom-up mktree -z непосредственных записей mode100644/blob либо tree. Хранить только нужные временные записи path/OID, без повторной копии входных bytes; порядок входа не влияет на tree. Успешный create_tree сам не pin.
- [x] **B5. Связать в lib.rs и зафиксировать GREEN.** `open_git_snapshots` явно вызывает GitCliAdapter::open и возвращает реализацию GitSnapshotPort. Реальные create_tree/read_blob делегируют ObjectDb; pin завершается в цикле C до task-done. Ровно service/ports/git.rs, adapters/git/* и lib.rs несут свои обязанности. Run B2 + A2 + текущие 30 root integration tests: PASS. Промежуточный локальный commit возможен; неполный компонент не публикуется и не объявляется TK-8 done.

### Цикл C — pin, closure и GC

- [x] **C1. Написать tests/git_retention.rs.** Оба object formats; собственные fixtures создают tree извне адаптера без user commit, а другой snapshot — через create_tree. Все новые tests используют публичные factory/trait:

```rust
let retained = git.pin(&s1).unwrap();
assert_eq!(retained.oid(), &s1);
assert_eq!(repo.direct_ref(s1.as_str()), s1.as_str());
assert_eq!(git.pin(&s1).unwrap().oid(), &s1);
repo.expire_reflogs_gc_prune_now_and_pack_refs();
let reopened = open_git_snapshots(repo.root()).unwrap();
assert_eq!(reopened.read_blob(&s1, &path).unwrap(), original_bytes);
assert!(repo.fsck_strict_no_reflogs().success());
```

Отдельные tests: `retained_snapshots_survive_gc` (inspect-like и external basis, nested closure, reopen); `unretained_unique_tree_is_collectable` (уникальный blob/tree без ref исчезает после prune); `incomplete_tree_and_ref_failure_do_not_pin` (missing/corrupt descendant, forced update-ref failure => InternalError, ref absent); `symbolic_and_conflicting_refs_are_rejected` (symbolic/corrupt value under expected name => error, user branch unchanged); `concurrent_pin_is_idempotent` (barrier starts two directly owned processes, both resolve exact same direct ref); `promisor_miss_never_fetches` (missing promised blob + reachable disposable remote/transport trap, explicit local protocol allow in repo does not override process deny, no fetch/helper marker and blob remains absent). Parent supervisor bounds fixture processes and reaps only owned children; no production timeout.

- [x] **C2. Зафиксировать RED.** Run `cargo +1.95.0 test --locked --offline --test git_retention -- --nocapture`. Минимальный compiling pin stub должен провалить successful pin/retention assertions; затем отдельный negative control без pin должен терять уникальный snapshot, а не проходить только из-за Git grace period. Сохранить реальные failures.
- [x] **C3. Реализовать удержание в refs.rs.** `Retainer::pin(&self, oid: &TreeOid) -> Result<RetainedTree, SnapshotError>`. Проверить root, затем обходить достижимые tree/blob, читая/проверяя OID каждого через ObjectDb; visited OIDs исключают повторную работу, Gitlinks не dereference/fetch, read gitlink как blob запрещён. Проверка candidate не сканирует все посторонние объекты repository. Проверить exact service ref посредством for-each-ref с форматом refname/objectname/symref и NUL fields; symbolic/другое значение => ошибка. Создать через update-ref --no-deref <ref> <oid> <zero_oid> (длина zero по format). При конфликте после CAS перечитать direct ref: только точное такое же значение означает конкурентный идемпотентный успех; прочие исходы — error. Повторный pin не меняет user refs/config/index. RetainedTree выдаётся лишь после подтверждённого ref; ref/closure failure не маскируется.
- [x] **C4. Зафиксировать GREEN и архитектурную проверку.** Run A2/B2/C2 и полный `cargo +1.95.0 test --locked --offline`. Проверить физические файлы, public/private API и imports по [общему порядку](../../zkit/letopis/architecture.md#program-structure-review): Command только в git/process.rs, service/domain не импортируют конкретный адаптер, root factory используется tests. `cargo +1.95.0 fmt --all --check` и `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings`: PASS.

### Платформы, evidence и завершение задачи

- [x] **V1. Добавить воспроизводимый Linux runner вместе с компонентом.** tests/git/Dockerfile использует `rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1`, устанавливает Git и rustfmt/Clippy при build, копирует src и все root tests/support/markdown fixtures, заранее cargo fetch --locked. .dockerignore явно пропускает tests/git_*.rs, tests/git/ и tests/git/Dockerfile; existing support/**/* уже включён. Runtime CMD выполняет версии/полный root suite/fmt/Clippy offline. README описывает реальные команды и scope.
- [x] **V2. Выполнить host и Linux проверки, сохранив полные логи.** Host: `cargo +1.95.0 test --locked --offline`; `cargo +1.95.0 fmt --all --check`; `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings`. Затем `docker build -f tests/git/Dockerfile -t letopis-tk8-check .` и `docker run --rm --network none letopis-tk8-check`. Образ не монтирует host compiled binaries. Дополнительный отдельный процесс со значениями GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE/GIT_OBJECT_DIRECTORY, указывающими на fixture trap, должен пройти Git integration test; helper Git setup очищает эти overrides. Записать фактические Git версии, image ID, counts, argv/exit codes и source/evidence SHA-256; числа тестов заранее не назначать.
- [x] **V3. Подготовить доказательства приёмки и итоговый commit.** Реальные TDD/negative controls, architecture review, bounds (частичный live capture; physical power loss/x86_64/resource limits не доказаны) в checks/TK-8. verification.json содержит hashes всех затронутых source/tests/config и logs; report сопоставляет каждый критерий TK-8 с именованными tests. Старые TK-7 receipts относятся к своему commit, не переписываются под новый root suite. После единственного независимого whole-branch review отразить fixes/проверки в review.md и ledger; не обещать второй review или будущий PASS. Карточка/EP1/roadmap получают done только после исправлений и финальных проверок; schedule.py --check остаётся 45 tasks/333h/34weeks. Коммитить только собственные paths, сохранить Engram. Merge/push/issue closure — отдельное решение пользователя.

## Самопроверка плана

- Spec coverage: X-3 create/read/pin, I-12 basis retention, raw bytes, OID/type/errors, unchanged index/HEAD/config, version/SHA256, protocol/GC/closure, module boundaries/lib composition и обе платформы указаны в циклах A/B/C/V.
- Все пять Review Focus имеют именованные tests и assertions; аппаратная durability и полный inspect/CLI сознательно не приписаны этому scope.
- Единственный public factory/trait contract определён выше; concrete/private names заданы в своих шагах. Частичные stubs допустимы лишь внутри TDD, не в финальной реализации.
- Один task выбран как минимальный законченный адаптер; подготовка окружения, fixtures, Linux verification и docs включены в его deliverable. План проверен автором, без дополнительного plan-review агента. Исправлен порядок проверки неизменности: FixtureState сравнивается до намеренной тестовой правки live-файла; signatures helpers/accessors и paths сверены.

## Первичные источники для решений process policy

- [Git 2.29 manual: GIT_ALLOW_PROTOCOL](https://git-scm.com/docs/git/2.29.0) — пустой allow-list запрещает любые transport protocols, даже когда repository config разрешает конкретный протокол. Это fallback старой версии для запрета fetch; поведение закрепляется тестом, ручное чтение manual не заменяет execution на Git 2.29.
- [Git cat-file](https://git-scm.com/docs/git-cat-file), [mktree](https://git-scm.com/docs/git-mktree), [update-ref 2.29](https://git-scm.com/docs/git-update-ref/2.29.0) — batch missing/type, NUL tree records, CAS/no-deref. Публичные ошибки и гарантия удержания проверяются локальными fixtures, а не выводятся из одного exit0.

## Результат исполнения

Subagent-driven; Task1 завершён. Продуктовый HEAD `92cf8bd`, evidence `b344c08`; 53 tests, fmt/Clippy PASS на macOS/Linux ARM64. После task review исправлен newline-root routing; после whole-branch review явно запрещён любой alternates path, чтобы pin не зависел от donor GC. Корни с не-UTF-8 именами остаются безопасно неподдержанными. [Приёмка](../../zkit/letopis/checks/TK-8/completion.md), [review](../../zkit/letopis/checks/TK-8/review.md), [журнал](../../zkit/letopis/checks/TK-8/execution-ledger.md). Исторические receipts сохранены. Merge/push/issue closure требуют отдельного решения пользователя.
