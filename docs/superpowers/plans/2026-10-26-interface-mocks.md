# Rust Interface Mocks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide library-generated mocks of applicable service interfaces and use them in all suitable existing tests across the repository.

**Architecture:** Generate mocks only under `cfg(test)` on application-owned traits; preserve production trait signatures and concrete implementations. Retain stateful fakes and real integration fixtures where they exercise state or scoped lifetimes.

**Tech Stack:** Rust pinned in `rust-toolchain.toml`, Mockall, Cargo, existing `make verify`.

**Spec:** `docs/superpowers/specs/2026-10-26-interface-mocks-design.md`.

## Global Constraints

- KISS, DRY, YAGNI and hexagonal boundaries from `AGENTS.md` apply; do not change product behavior to ease mocking.
- Pin the test-only dependency and lock it; project checks run offline.
- Respect existing associated types and lifetime contracts. Keep stateful fakes and real integration fixtures where useful.
- Tests remain independent, readable, and use arrange/act/assert; manually mutation-check changed behavior.

## Review Focus

- Zero-work and busy-lock paths must not launch a process; Task 1 tests call counts.
- Ready work launches once only after releasing the lock; Task 1 retains the ordering fixture and checks a mock launch.
- Readiness and drain failures propagate safely; Task 1 retains error checks and tests delegation.
- Stateful storage lifetimes and cross-process semantics remain real; Task 1 preserves fixtures.
- Normal production compilation must not require test-only Mockall macros; Task 2 checks release/doc targets.

### Task 1: Generate and exercise interface mocks

**Files:** `Cargo.toml`, `Cargo.lock`, `src/service/command_traits.rs`, `src/service/background.rs`.

**Interfaces:** `EmbeddingExecutor::{has_work,drain}`, `DrainLauncher::spawn_drain`; generated `MockEmbeddingExecutor`, `MockDrainLauncher` are test-only.

- [x] Write a focused test using generated executor/launcher mocks for no-work (one readiness call, no spawn) and run it to confirm it fails to compile without mocks.
- [x] Pin Mockall as a dev-dependency, regenerate the lockfile, fetch required packages and add conditional generation to these interfaces. Run the focused test offline and confirm success.
- [x] Inventory hand-written test implementations across all Rust tests; migrate suitable interaction doubles to mocks while retaining meaningful behavior coverage. Cover no-work, launch, propagation/delegation and relevant argument/call assertions; retain real lock-release ordering checks, stateful fakes and integration fixtures. Generate additional interface mocks when a test uses them; document retained cases and reasons.
- [x] Run the focused background tests offline; temporarily suppress one covered call and verify the matching test fails, then restore and rerun successfully.

### Task 2: Document and verify

**Files:** `docs/development.md`, this plan, and the files from Task 1.

- [x] Document how to use `#[cfg_attr(test, mockall::automock)]`, expectations, and when to prefer stateful fakes or integration fixtures, including scoped-storage limitations.
- [x] Run `make verify` in the final worktree with the project-required permissions and prepared offline dependencies; record exact results and limitations.
- [x] Review `git diff`, production dependency separation, trait visibility and design compliance; check docs and update issue with progress, keeping it open until completion is established.

## Execution notes

- Baseline `make check` passed outside the process-restricting sandbox. The first sandboxed run failed with `PermissionDenied` in subprocess/lock tests; it did not establish a product defect.
- The suite inventory covered test-side trait implementations across `src/` and `tests/`. Simple interaction doubles in CLI dispatch, background execution, search, refresh and status were migrated. Real process/SQLite wrappers, graph pagination, fault-injection, lock-lifetime and shared-event stateful fixtures remain where mocks would hide the behavior under test.
- `automock` cannot generate the current borrowed `Embedder` and `EmbeddingProvider` signatures; test-local `mock!` declarations with named lifetimes compile and retain the real production signatures. External integration tests also use a local `mock!` because the library's `cfg(test)` mocks are not exported there.
- Seven behavioral mutations were detected and restored: readiness decision, reindex dispatch, check paths, search arguments, traversal depth, captured query, and provider validation. Logs are in `/private/tmp/letopis-mutation-*.log`.
- Independent review identified a missed search double and helper ordering. The search double was converted using the test-local explicit-lifetime macro, and the helpers were moved after their callers. No production boundary or clear coverage loss was found in the reviewed changes.

- Final `make verify V=1` passed on macOS with Rust 1.95.0: debug and release each 61 direct test targets, 777 passed, 0 failed, 5 ignored; strict Rustdoc and Clippy passed. The full log is preserved at `.artifacts/interface-mocks/verification.log`, SHA-256 `bf37c527131089904d68b08cdc2b653f065f285e498eae86a37d209d167c84ef`. Seven mutation logs are saved beside it. Linux was not run in this worktree.
- At the time of verification, the branch was uncommitted and unmerged; GitHub issue #50 was open.

- Follow-up, 2026-10-27: added Given / When / Then comments to all 844 Rust and 141 Python unit/integration tests, including standalone harness tests, and recorded the convention in `AGENTS.md` and `docs/development.md`. The audit compared Rust tokens and Python ASTs to a pre-change snapshot and found no non-comment changes, missing labels, or first Then-before-When markers. A semantic spot review corrected misplaced and mixed labels; it did not exhaustively review every boundary.
- Follow-up `make verify V=1` passed on macOS: debug and release each 61 direct targets, 777 passed, 0 failed, 5 ignored; Clippy and strict Rustdoc passed. The log SHA-256 is `c475fd25130b5df00167a9c92e4dd009e12fe99857772a78d9a4fced0a2cd15c` under `.artifacts/given-when-then-20261027/`. Historical standalone and Python test runners were not run for the comment-only changes; Linux was not run.
