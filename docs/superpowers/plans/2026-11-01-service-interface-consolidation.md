# Consolidate application interfaces in service/mod.rs

## Approved intent

The user explicitly requested putting everything interface-related in
`src/service/mod.rs` and retaining concrete application services in their own files.
This supersedes guidance allowing private trait files and the usual file-size target
for this central contract file. Public paths, signatures, behavior, transaction
lifetimes and composition remain unchanged.

## Implementation

1. Move the existing declarations from `storage.rs`, `document_traits.rs`,
   `snapshot_traits.rs`, `embedding_traits.rs` and `command_traits.rs` into
   `service/mod.rs`. Preserve documentation and trait/method/associated-type contracts.
   Include the existing three snapshot boundary data types; do not create new files.
2. Remove the five empty modules and redundant reexports. Merge imports, adjust
   `InitFiles`'s supertrait path and direct test mock imports. Keep concrete services
   and other existing data/error files separate.
3. Synchronize AGENTS.md, development and architecture guidance. Preserve dated
   decisions, reports and compatibility fixtures. Extend the native-frame test with
   the current consolidated trait path, retaining the historical frame.
4. Verify declarations mechanically, mutation-check the added frame case, run
   `make fmt`, `git diff --check` and `make verify`, validate affected Markdown links,
   and review the final change. No native-storage runner is needed for moving trait
   declarations; storage technology, native stack, build and schemas are untouched.

## Workspace boundary

Base: c3de57b on issue-69. Four pre-existing modified files belong to the user:
`src/service/document_refresher/tests/basis.rs`, `src/storage/read.rs`,
`src/storage/schema.rs`, `src/storage/sqlite.rs`. Preserve their exact bytes and
verify them against captured SHA-256 hashes. Do not commit, merge or publish.


## Execution record

- All five declaration groups match the original source ignoring whitespace and
  correcting only the relocated `InitFiles` supertrait path. Exactly 34 public
  traits are declared directly in `service/mod.rs`.
- The new native-stack frame case passes. A compiled mutation restricting the
  observer to the old private module path fails the test. Source restored exactly.
- All-target cargo check passed. Fresh review found no interface-consolidation
  defects; module paths, DTOs, mock attributes and signatures remain intact.
- Concurrent external edits renamed BackgroundService/background.rs to
  BackgroundRunnerService/background_runner.rs and modified lib.rs, subsystem-map
  and historical documents. These edits were not made by this task and are retained.
  The first make verify attempt stopped at formatting in the renamed file; apply
  formatting and repeat the final gate. No source behavior is changed by that fix.
- Review identified historical names rewritten by those external changes in TK-11,
  TK-28 reports and the TK-14 spec. This is reported separately; this task does not
  revert concurrent edits owned by others.
- The user's separate request to simplify generics is under discussion. The choice
  between reducing service parameters and redesigning transaction APIs remains open;
  consolidation itself changes neither generic parameters nor transaction contracts.

## Final verification

- `make fmt`: passed. `git diff --check`: passed.
- `cargo check --locked --offline --all-targets`: exit 0.
- `cargo test --locked --offline --test cli_cancellation git_read_observer_recognizes_debug_and_inlined_release_frames`: one passed test.
- Compiling old-namespace mutation: test failed as intended; source restored byte-for-byte.
- Final `make verify`: exit 0, output `ok`; includes all-target checks, Clippy,
  root debug tests, strict Rustdoc and root release tests.
- All five moved declaration groups preserved, ignoring whitespace and the necessary
  `InitFiles` supertrait qualifier adjustment. Exactly 34 public traits are centralized.
- All four original user-modified files retain their captured SHA-256 hashes.
  All 299 source inputs captured at the early verification checkpoint still match.
- Linux and separate storage/native runners were not run for this declaration move.
  Concurrent storage changes are outside the scope of this refactor's native claims.
- No commits, merges or publishing performed. External historical-document review
  finding remains outside this task's ownership and is not represented as resolved.

The next design choice is documented separately in
[the unapproved simplification proposal](../specs/2026-11-01-service-types-simplification.md).
