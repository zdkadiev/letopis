# Flatten service files and inline unit tests

## Summary

Remove all subfolders under `src/service/`. Keep each service's implementation
and unit tests in its existing `.rs` file. Preserve behavior and public APIs.

## Changes

- Move the 33 nested Rust files into their owning service files.
- Replace external test declarations with inline `#[cfg(test)] mod tests { … }` blocks.
- Preserve nested test modules, names, attributes and visibility, including private
  access and exact subprocess test selectors.
- Move production helpers into inline private modules within the owning file,
  preserving existing module boundaries and imports.
- Remove obsolete file-path attributes and emptied directories. Do not use
  `include!` or replacement folders.
- Keep production code before tests. Preserve test bodies and existing comments;
  leave the broader test-comment migration outside this change.
- Update current guidance to record inline service tests as the agreed exception
  to the usual file-length recommendation. Preserve historical documents.

## Verification

- Capture the unit-test listing before editing; compare names and ignored status
  afterward to detect lost or renamed tests.
- Confirm no service subdirectories or external child-module declarations remain
  outside `service/mod.rs`, which still declares the service files.
- Check module visibility, imports and subprocess selectors.
- Run `make fmt`, `git diff --check`, and `make verify`.
- Check links in changed documentation. Storage and Linux runners are unnecessary
  for this file-layout-only change.

## Assumptions

- Existing nested modules remain inline; combining namespaces is outside scope.
- Larger files are accepted, including approximately 2,500 lines with tests.
- Preserve unrelated staged `examples/transaction_cost.rs` changes.
- Leave resulting changes uncommitted for review.

## Execution record

- Implement in the supplied `issue-69` worktree, preserving its unrelated index
  and working-tree state. This plan is the approved scope; no separate spec.
- Use existing behavioral tests and before/after listings for this mechanical
  move; no new behavior or test bodies require a red/green or mutation cycle.
- Baseline and verification artifacts: `/tmp/letopis-flatten-service-20261101/`.

- Clippy identified the existing test-only `reach_checkpoint` reexport after the
  new inline tests. Move it before the test block, preserving its visibility
  and `cfg(test)` gate, to satisfy the tests-last rule.
- Flattened all 33 nested Rust files; retained private production helper modules
  and every nested test namespace, attribute and visibility.
- Compared library listings: 463 test names and 9 ignored test names unchanged.
  Both exact subprocess child selectors remain unchanged.
- Checked all 179 local link paths in changed guidance. No service directories,
  external child declarations outside `service/mod.rs`, obsolete path attributes
  or `include!` replacements remain.
- Independent review compared 62 original module/file bodies, including comments
  and literals, and found only formatting changes. The reexport placement fix
  received a follow-up review with no findings. Composition and central service
  interfaces remain unchanged.
- The unrelated staged example patch and its absent working-tree file are
  preserved. Storage and Linux runners are outside this layout-only scope.

## Added scope: CLI response and Git

The user additionally requested the same flattening for `src/cli/response/`
and subdirectories of `src/git/`.

- Inline the two CLI response test files into `src/cli/response.rs`.
- Inline the eight nested Git test files into their existing owning `.rs` files.
- Preserve nested namespaces, visibility, attributes, comments and exact
  subprocess selectors. Keep `git/mod.rs` declarations and reexports unchanged.
- Remove emptied directories and document the corresponding file-length exception.
- Recompare the complete library and ignored-test listings, check changed links,
  run `make fmt`, `git diff --check` and `make verify` for the combined tree.
- This remains a file-layout-only change; no native storage/Git stack, contracts,
  build inputs or behavior change, so platform proof runners are unnecessary.
- The service-only run passed debug tests and strict Rustdoc; its release phase
  was interrupted deliberately when this additional scope arrived. Final
  completion requires a new full verification of the combined changes.
- Added scope implemented: all 10 nested CLI/Git test files are inline; the
  response subdirectory and every Git subdirectory are gone. Combined totals:
  43 moved files, 463 unchanged library test names, 9 unchanged ignored names.
- Independent added-scope review compared 25 module/file bodies and found no
  actionable issues; test blocks are last, namespaces/visibility/comments and
  literals are retained, and `git/mod.rs` is unchanged.
- Combined layout, subprocess-selector and documentation-link checks passed.
- Combined Clippy found `semicolon_if_nothing_returned` in a Git test closure
  expanded into a block by rustfmt after indentation changed. Added the required
  semicolon to the assignment; its unit result and behavior are unchanged.

## Final verification

- `make fmt`: passed.
- `git diff --check`: passed.
- `make verify`: passed (exit 0, `ok`), covering formatting, all-target checks,
  Clippy, debug root tests, strict Rustdoc and release root tests on the combined
  service/CLI/Git tree. Log: `/tmp/letopis-flatten-service-20261101/make-verify-final.log`.
- Final library listings: all 463 names and all 9 ignored names exactly match the
  baseline. Exact subprocess selectors and module visibility are preserved.
- Final layout and changed-document local-link checks: passed.
- Independent reviews and scoped follow-up reviews: no open findings.
- Unrelated staged example and absent working-tree file preserved; all task
  changes remain uncommitted. No platform proof runners were rerun.
