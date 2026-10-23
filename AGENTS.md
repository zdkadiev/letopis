# Agent instructions

Work as a senior software engineer: understand the affected code and contracts, make focused changes, and keep the implementation simple, idiomatic, and maintainable.

## Start with the project documents

- Read the relevant task card under `docs/zkit/letopis/roadmap/epics/*/tasks/` before implementing a roadmap task.
- Treat [architecture](docs/zkit/letopis/architecture.md#program-structure), [architecture review](docs/zkit/letopis/architecture.md#program-structure-review), [decisions](docs/zkit/letopis/decisions.md), and the relevant [contracts](docs/zkit/letopis/contracts/) as the design sources. Read [mechanics](docs/zkit/letopis/mechanics.md) for affected algorithms and invariants.
- Keep agreed specifications and implementation plans in `docs/superpowers/specs/` and `docs/superpowers/plans/`. When the user requests Superpowers, follow its applicable workflow and preserve the resulting decisions in those documents.
- Distinguish accepted decisions, proposals, and open questions. Do not silently replace an agreed design with a new runtime, dependency, process, or abstraction. Prefer KISS and YAGNI.

## Hexagonal architecture and physical modules

The directory structure must reflect responsibilities and dependency boundaries:

- `src/domain/`: pure entities, values, and rules; no filesystem, process, Git, SQL, HTTP, or CLI orchestration.
- `src/service/`: application-owned traits and concrete application services. Define small interfaces in `service/mod.rs`; keep each concrete service struct and its implementations in a separate responsibility-named file. Services depend on domain types and their interfaces, not concrete technology implementations.
- `src/git/`, `src/storage/`, `src/embedding/`, `src/markdown/`, `src/filesystem/`, and `src/process/`: separate responsibility-named modules, introduced when their tasks implement them. Concrete structs implement the application-owned traits. These modules do not orchestrate one another.
- `src/cli/`: argument parsing, dispatch, command handlers, and output formatting. Handlers convert input into typed requests and invoke service interfaces; business rules belong to services/domain. Handlers do not call storage/Git/embedding adapters directly.
- `src/lib.rs`: composition root with actual constructor calls wiring adapters into services and services into handlers through interfaces. Module declarations and reexports alone are not composition.
- `src/main.rs`, when introduced: thin CLI entry point using the assembly from `lib.rs`.

Use `trait` for component interfaces and concrete `struct`/`impl` for implementations. Name types by responsibility or technology (`Storage`, `SqliteStorage`, `GitSnapshots`, `GitCli`), without `Port`/`Adapter` suffixes. Do not introduce `ports/`, `adapters/`, or `contracts/` directory layers. A module's `mod.rs` defines its interface, declares children, and explicitly reexports the necessary types. Put each concrete implementation's struct, inherent methods, and trait implementations together in its own file, for example `git/cli.rs` or `storage/sqlite.rs`; keep implementation bodies out of `mod.rs`. If interfaces grow, split them by responsibility into private files and reexport them from `mod.rs`. Keep implementation details private and expose only the necessary API. Add modules with real functionality; do not scaffold future subsystems or trivial forwarding services. These rules describe the target layout; the TK-9 plan must include moving the existing Git/Markdown modules and updating their imports.

Shared `src/utils.rs` and private module-local `utils.rs` are allowed for useful helpers; see [code reading order](#code-reading-order) for when to extract them. Keep domain rules and technology-specific behavior in their owners; utilities must not bypass module boundaries.

## Rust conventions

- Follow the pinned toolchain in `rust-toolchain.toml` and lint policy in `Cargo.toml`; [development guide](docs/development.md) explains the checks.
- Import types with `use` instead of repeating long `crate::...` paths at call sites. Write enum variants through the imported type, for example `SnapshotErrorKind::InternalError`. Prefer explicit imports over wildcard imports.
- Name custom functions and methods with verbs or verb forms describing their action: `read_entries`, `build_command`, `verify_object`, `get_kind`, `is_empty`, `has_exact_ref`. Use `get_` for custom accessors rather than bare nouns. Keep established Rust constructor/conversion forms (`new`, `new_with_cause`, `as_str`, `to_hex`, `into_inner`) and names required by implemented traits (`fmt`, `source`, `drop`). Apply the rule to test helpers; test names describe the behavior being verified.
- Replace meaningful repeated domain/protocol literals with clearly named constants near their owning module; use SCREAMING_SNAKE_CASE. Do not replace literals mechanically or collect unrelated constants in a global bag.
- Keep functions and methods focused: aim for about 40 lines, with 60 lines as the upper limit. Split longer implementations into cohesive named helpers, not arbitrary wrappers or compressed formatting. Apply the same rule to tests and test helpers.
- Use ownership and borrowing deliberately; add cloning, shared ownership, locks, or dynamic dispatch only for an actual need.
- Choose dispatch in this order: a concrete type, then a generic bound, then `dyn Trait`. Use `dyn` only where a generic cannot work, such as an optional collaborator (`Option<&mut dyn Trait>`, where `None` has no concrete type), a type-erased error cause (`Box<dyn Error + Send + Sync>`), a stored test hook (`Box<dyn FnOnce()>`), or a generic parameter list that would spread through public signatures.
- Do not use `unwrap`/`expect` in production code; return a typed `Result`. `expect` is allowed only for an invariant guaranteed by construction; mark it `#[expect(clippy::expect_used, reason = "...")]` and state the invariant in the reason. Tests may use both. A detected programming bug is a panic, an expected failure is an error.
- Every public type implements `Debug`.
- Model finite alternatives with enums and expected failures with typed `Result`. Document public APIs as described in [readability and documentation](#readability-and-documentation). Preserve useful internal error causes while keeping public diagnostics free of secrets and raw knowledge/provider/subprocess payloads.
- Compose ordinary storage queries with SeaQuery and typed table/column identifiers; execute through rusqlite using bound values from the private storage/query.rs helper. Keep builder types inside storage. Raw SQL is reserved for schemas, SQLite introspection/PRAGMAs/native probes, and specialized FTS5/vec1 search or native commands; format substantial SQL across readable lines. Never interpolate user values or render them into SQL with to_string.
- Keep I/O synchronous according to the design; do not add an async runtime without an agreed requirement. Product Git commands belong in the Git process adapter and use argv/stdin, never shell interpolation.
- Standard Clippy checks and the explicitly selected pedantic checks in `Cargo.toml` are enabled; see the development guide for each rule and its purpose. `dbg!`, `todo!`, and `unimplemented!` are denied. Do not enable all of `restriction`, `pedantic`, or `nursery` indiscriminately. Fix the underlying finding rather than adding a blanket allowance; any exception must be narrow and explain its reason, written as `#[expect(lint, reason = "...")]` rather than `#[allow]`, so the exception fails the build when it is no longer needed. Public fallible APIs document `# Errors`, and public APIs that can panic document `# Panics`.

## Code reading order

Code reads top to bottom, from abstract to concrete. Apply these rules to new and touched code only; do not reorder untouched code just to conform.

- A caller is defined above everything it calls. A function or method must not call one defined above it. Type and constant definitions are not calls and may be referenced from above.
- Group by level of abstraction: orchestration and abstract calls first, then intermediate steps, then low-level details. Keep methods of the same level adjacent.
- Within an `impl`, public methods come first and private helpers last, in the order they are first called. A public method that calls another public method stays above it.
- Within a file, order items as constants, types, `impl` blocks with the public API, trait implementations, then private free functions. Trait implementations such as `Display` stay next to their type. `#[cfg(test)]` modules go last, with test helpers after the tests.
- Place a helper by its consumers: one file keeps it as a private function at the bottom of that file; several files of one module share the module-local `utils.rs`; several modules share `src/utils.rs`. Extract only when there are at least two consumers. `utils.rs` files follow the same top-to-bottom order.

## Readability and documentation

Apply these rules to new and touched code only.

- Prefer early exits: `let Some(x) = value else { return ... };` and `?` over nested `match` and `if let`. Keep nesting to two levels.
- Use newtypes and enums instead of bare primitives: `SnapshotId(String)` rather than `String`, an enum rather than `bool` flags.
- Group more than three or four parameters into a request struct.
- Do not use `&mut` parameters as output: a function returns its result instead of filling an argument (`fn count(..) -> Counts`, then merge at the caller). `&mut self` for state the type owns is fine. Allowed exceptions: signatures fixed by a trait or external API (`fmt`, `Read`/`Write` streams, `rusqlite::Connection` transactions) and handles that are the resource itself, not a result container. Apply to new and touched code only.
- Use iterator chains while they read well (three or four steps); split longer chains into named steps instead of a manual loop or one long expression.
- Write explicit types in signatures; do not use `impl Trait` where it hides meaning.
- Keep files focused on one responsibility, roughly 300-500 lines; split a file that grows far beyond that.
- Comments explain why, not what: required for `unsafe`, platform workarounds, and non-obvious invariants. Do not narrate the body line by line or repeat the method name.
- Document every `pub` and interface method (traits in `service/mod.rs`, `mod.rs` re-exports) and every private method whose purpose is not clear from its name and signature. Trivial accessors and obvious one-liners need no comment.
- A method comment starts with one sentence saying what it does, then states the contract: preconditions, what is returned, side effects, and guarantees such as atomicity or idempotence, plus `# Errors` and `# Panics` where they apply.
- Document a trait's contract on the trait itself: what every implementation must guarantee, not only the current one. An `impl` comment covers only what is specific to that implementation, such as SQLite or Git behavior.
- Update a comment in the same commit that changes the behavior it describes.

## Verification

Run commands from the repository root; from elsewhere use `make -C <path> <target>`. Dependencies are locked and offline: if they have not been prepared, run `cargo fetch --locked` first. Checks never download dependencies or rewrite files; apply formatting with `make fmt`. `make check` and `make verify` print one `ok` line on success and the full output of the failed step on failure, which keeps logs small; `V=1` (for example `make check V=1`) shows every command's output.

| Command | Runs | Use |
| --- | --- | --- |
| `make check` | `cargo fmt --check`, `cargo check` (all targets), Clippy with `-D warnings`, root tests | Required for every product Rust change. |
| `make verify` | `make check`, strict Rustdoc (`RUSTDOCFLAGS='-D warnings'`), root tests in the release profile | Before claiming completion of a task that changes `src/`, public documentation, or dependencies. |
| `python3 -B tests/storage/run.py --platform host\|linux --evidence <new directory>` | Schema parity, runner tests, native SQLite/vec1 capabilities, copied standalone executable and its linkage | When storage, native stack, build, or vendored inputs change. Evidence goes to a new directory, never over existing evidence. |

### Review and reporting

- A test must not assert its own ground truth: do not compute the expected value with the code under test or repeat the implementation in the assertion. Use independent literals or an independent oracle.
- A `TODO` names an issue (`// TODO(#123): ...`); otherwise fix it now or record it in the task card.
- Mutation-check the tests you write: after a test passes, temporarily break the code it covers by hand (flip a condition, change a returned value, remove a call) and confirm the test fails, then restore the code. A test that still passes does not verify that behavior; strengthen it. No tooling is required; do not leave mutations in the tree.
- Structure tests as Arrange, Act, Assert: prepare inputs/fixtures, execute the behavior, then check results. Use explicit phase comments where they clarify boundaries; repeat phases for genuinely sequential scenarios. Keep helpers private and keep scenario assertions visible.
- Test meaningful behavior and failure paths for behavioral changes. Import-only or documentation-only edits do not need new behavioral tests.
- Before claiming completion, verify the actual changed tree. Report exact commands/results and any unrun checks or platform limits.
- Review physical files, visibility, trait implementations, import direction, and `lib.rs` wiring; a passing linter alone does not establish architectural compliance.
- Root checks do not execute historical proof harnesses with their own manifests. Run the relevant harness or platform runner when the change requires it; do not imply unrelated suites were rerun.
- Preserve historical evidence against its captured revision. Store new evidence separately and update the relevant task/spec/plan with findings, decisions, and limits. For documentation-only edits, check affected links and consistency rather than rerunning unchanged product suites.

## Workspace and memory

- Inspect the current branch/worktree and dirty files first. Preserve user edits and unrelated work; do not stash, reset, overwrite, or clean them away.
- Keep changes in the intended task worktree. Commit only the intended paths. Merge, push, publish, and update GitHub issues when authorized by the user; preserve authorization already given in the conversation.
- Use the existing Engram store through its CLI: start with `engram status`, retrieve relevant prior constraints, and record useful decisions, findings, changes, and verification. Worktrees reuse the project's existing store through the appropriate `ENGRAM_DB`; do not initialize a second store implicitly.
- Never edit Engram's database directly or commit/discard another writer's dirty graph. Preserve existing attributes on updates and keep historical provenance accurate.
