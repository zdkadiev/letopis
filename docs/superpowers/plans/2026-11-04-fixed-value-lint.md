# Fixed-value helper check implementation plan

> **For agentic workers:** Use Superpowers subagent-driven development; validate each task independently and review the resulting tree. Steps use checkbox syntax.

**Goal:** Reject narrowly defined fixed-error and fixed-response helpers while retaining meaningful builders and error mappings.

**Architecture:** Parse repository Rust files with a development-only `syn`-based checker. Match only the conservative patterns in the spec; clean existing violations before enabling the repository-wide check. Keep runtime dependencies and product behavior unchanged.

**Tech stack:** Rust 1.95.0, existing locked `syn` 2.0.119, Cargo, Makefile.

**Spec:** [Fixed-value helper check](../specs/2026-11-04-fixed-value-lint-design.md). User requested fixing all audit findings, selecting whole-repository cleanup.

## Global constraints

- Scan `src/`, `tests/`, and `examples/`, without following symlinks; fail on parse/read errors.
- Preserve error kinds, causes, lazy evaluation and user edits. Named constants may hold repeated messages.
- Do not flag trait methods, `new`/`default`, parameterized functions, meaningful builders, or arbitrary calls. Document unsupported macros and response types.
- Keep each query and trait contract stable except for separately requested naming fixes. Use offline locked dependencies and no product runtime dependency.
- New tests use described Given/When/Then phases and must be mutation-checked.

## Review focus

- Fixed error constructor, `Err` wrapper, and fixed `CommandResponse` must be detected, including a constant message and fixed I/O cause.
- Parameter-dependent mappings and meaningful builders must not be flagged.
- Trait implementations, trait defaults, and ordinary `new`/`default` constructors must remain valid.
- Comments, raw strings, macros, unreadable files, malformed Rust, and symlinks must not hide errors or produce spurious source findings.
- Existing fixed-error call sites retain exact messages, kinds, causes and laziness after cleanup.

### Task 1: Implement syntax checker

**Files:** Create `examples/fixed_value_lint.rs`; modify `Cargo.toml`, `Cargo.lock`, and `Makefile`.

- [ ] Add failing checker tests for the positive and negative cases in the spec, including nested wrappers and parse/read errors.
- [ ] Implement the smallest syntax visitor and deterministic filesystem traversal that passes the tests. Keep fixtures in memory, outside scanned Rust files.
- [ ] Run the checker tests, then mutate the positive matcher and confirm an appropriate test fails; restore it.
- [ ] Integrate the checker and its tests into `make check`, keeping dependencies locked/offline.

### Task 2: Remove current violations

**Files:** Existing Git, filesystem, CLI and service implementation files identified by the audit.

- [ ] Replace fixed-error/fixed-response helpers with explicit constructions at each use, using closures for lazy combinators. Preserve existing error messages and causes; use constants only for meaningful repeated literals.
- [ ] Run targeted tests for each affected area. Run the checker over the whole tree and resolve findings without introducing blanket allowances.
- [ ] Inspect the actual diff for user changes, trait implementations, and boundary compliance.

### Task 3: Verify and review

**Files:** `docs/development.md`, affected Letopis records and the final diff.

- [ ] Document the checker, its conservative detection limits, and validation commands.
- [ ] Run `make verify` and the host storage runner with a fresh evidence directory when build inputs change.
- [ ] Obtain independent review and fix findings; report exact checks, remaining limitations and uncommitted state.
