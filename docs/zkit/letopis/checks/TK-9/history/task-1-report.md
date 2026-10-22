# Task 1 implementation report

Status: DONE

## Scope and files

Migrated src/adapters/git to src/git, with GitCli struct/inherent methods/GitSnapshots impl together in cli.rs and unit tests in cli/tests.rs. git/mod.rs only declares children and explicitly reexports GitCli within the crate. GitCli fields are pub(super) for existing sibling helpers; constructor visibility is crate-only. Fixture path remains ../../../tests/support/git.rs and subprocess test selectors use the new module prefix.

Migrated src/adapters/markdown to src/markdown, extracting parse and parsed values into parser.rs and Diagnostic/DiagnosticKind into diagnostic.rs. Diagnostic::new_at is pub(super) for sibling parsing helpers. mod.rs declares/reexports the public API and shared event type.

Moved GitSnapshots/SnapshotFile into service/mod.rs; SnapshotError/Kind and RetainedTree with their impls live in snapshot_error.rs and retained_tree.rs. Removed obsolete adapters/ports wrappers. lib.rs uses imported types and constructs GitCli::open, exposing only impl GitSnapshots. Updated six integration/support consumer files. No aliases, behavior changes, storage scaffolding or new tests.

## RED

Command: cargo check --locked --offline --all-targets

After updating consumer imports first, compiler failed as intended with E0432: unresolved import `letopis::markdown`, and unresolved `letopis::service::{GitSnapshots, SnapshotError, SnapshotErrorKind, SnapshotFile}`. It also reported E0599 because new trait imports were unavailable. Full actual compiler output: /tmp/tk9-task1-evidence/red.txt. The capture shell returned zero after printing the log; compiler failure is established by its explicit `error: could not compile` diagnostics.

## GREEN and comparison

Command: ./scripts/check-rust.sh

Passed formatting, all-target compiler check, Clippy, all 53 root scenarios (7 unit + 46 integration), and 0 doctests. Full output: /tmp/tk9-task1-evidence/green.txt.

Before/after command: cargo test --locked --offline -- --list. Captured baseline before any source mutation. Comparison passed: exactly 53 scenarios, with only adapters::git::tests:: -> git::cli::tests:: normalization. Baseline and after lists, initial SHA-256 source/test hashes, scenario comparison and unchanged helper token comparison are under /tmp/tk9-task1-evidence/. All eight moved Git/Markdown helper files have identical whitespace-normalized token content after agreed name/path substitutions.

Command: rg -n 'adapters::|service::ports|GitSnapshotPort|GitCliAdapter' src tests/*.rs tests/support

No matches (exit 1 as expected). Historical proof sources/docs were not changed or rerun. No platform-wide storage/static-link claims apply to this mechanical task.

## Self-review

Reviewed physical files, service-owned trait signatures, explicit exports/imports, inward dependency direction and actual lib.rs construction. Function bodies, error classifications and snapshot retention behavior are preserved. Domain files and test scenario bodies are unchanged. The preexisting long open_with_process constructor remains unchanged deliberately to keep migration mechanical; no incidental refactor. One intermediate check caught GitCli visibility too narrow for its crate reexport; corrected struct visibility to pub(crate), leaving fields pub(super), then reran full checks successfully.

Only src/ and the six changed integration/support files are committed. Parent-owned docs/checklists and this report remain outside the source migration commit. No outstanding concerns.
