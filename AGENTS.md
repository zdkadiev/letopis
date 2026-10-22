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
- `src/git/`, `src/storage/`, `src/embedding/`, `src/markdown/`, and `src/filesystem/`: separate responsibility-named modules, introduced when their tasks implement them. Concrete structs implement the application-owned traits. These modules do not orchestrate one another.
- `src/cli/`: argument parsing, dispatch, command handlers, and output formatting. Handlers convert input into typed requests and invoke service interfaces; business rules belong to services/domain. Handlers do not call storage/Git/embedding adapters directly.
- `src/lib.rs`: composition root with actual constructor calls wiring adapters into services and services into handlers through interfaces. Module declarations and reexports alone are not composition.
- `src/main.rs`, when introduced: thin CLI entry point using the assembly from `lib.rs`.

Use `trait` for component interfaces and concrete `struct`/`impl` for implementations. Name types by responsibility or technology (`Storage`, `SqliteStorage`, `GitSnapshots`, `GitCli`), without `Port`/`Adapter` suffixes. Do not introduce `ports/`, `adapters/`, or `contracts/` directory layers. A module's `mod.rs` defines its interface, declares children, and explicitly reexports the necessary types. Put each concrete implementation's struct, inherent methods, and trait implementations together in its own file, for example `git/cli.rs` or `storage/sqlite.rs`; keep implementation bodies out of `mod.rs`. If interfaces grow, split them by responsibility into private files and reexport them from `mod.rs`. Keep implementation details private and expose only the necessary API. Add modules with real functionality; do not scaffold future subsystems or trivial forwarding services. These rules describe the target layout; the TK-9 plan must include moving the existing Git/Markdown modules and updating their imports.

Shared `src/utils.rs` and private module-local `utils.rs` are allowed for useful helpers. Keep domain rules and technology-specific behavior in their owners; utilities must not bypass module boundaries.

## Rust conventions

- Follow the pinned toolchain in `rust-toolchain.toml` and lint policy in `Cargo.toml`; [development guide](docs/development.md) explains the checks.
- Import types with `use` instead of repeating long `crate::...` paths at call sites. Write enum variants through the imported type, for example `SnapshotErrorKind::InternalError`. Prefer explicit imports over wildcard imports.
- Name custom functions and methods with verbs or verb forms describing their action: `read_entries`, `build_command`, `verify_object`, `get_kind`, `is_empty`, `has_exact_ref`. Use `get_` for custom accessors rather than bare nouns. Keep established Rust constructor/conversion forms (`new`, `new_with_cause`, `as_str`, `to_hex`, `into_inner`) and names required by implemented traits (`fmt`, `source`, `drop`). Apply the rule to test helpers; test names describe the behavior being verified.
- Replace meaningful repeated domain/protocol literals with clearly named constants near their owning module; use SCREAMING_SNAKE_CASE. Do not replace literals mechanically or collect unrelated constants in a global bag.
- Keep functions and methods focused: aim for about 40 lines, with 60 lines as the upper limit. Split longer implementations into cohesive named helpers, not arbitrary wrappers or compressed formatting. Apply the same rule to tests and test helpers.
- Use ownership and borrowing deliberately; add cloning, shared ownership, locks, or dynamic dispatch only for an actual need.
- Model finite alternatives with enums and expected failures with typed `Result`. Document public APIs. Preserve useful internal error causes while keeping public diagnostics free of secrets and raw knowledge/provider/subprocess payloads.
- Compose ordinary storage queries with SeaQuery and typed table/column identifiers; execute through rusqlite using bound values from the private storage/query.rs helper. Keep builder types inside storage. Raw SQL is reserved for schemas, SQLite introspection/PRAGMAs/native probes, and specialized FTS5/vec1 search or native commands; format substantial SQL across readable lines. Never interpolate user values or render them into SQL with to_string.
- Keep I/O synchronous according to the design; do not add an async runtime without an agreed requirement. Product Git commands belong in the Git process adapter and use argv/stdin, never shell interpolation.
- Standard Clippy checks and the explicitly selected pedantic checks in `Cargo.toml` are enabled; see the development guide for each rule and its purpose. `dbg!`, `todo!`, and `unimplemented!` are denied. Do not enable all of `restriction`, `pedantic`, or `nursery` indiscriminately. Fix the underlying finding rather than adding a blanket allowance; any exception must be narrow and explain its reason. Public fallible APIs document `# Errors`, and public APIs that can panic document `# Panics`.

## Verification

For product Rust changes run:

```sh
./scripts/check-rust.sh
```

It checks formatting, compiler diagnostics, Clippy, and root tests with locked offline dependencies. If dependencies have not been prepared, use `cargo fetch --locked` first. Formatting can be applied with `cargo fmt --all`.

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
