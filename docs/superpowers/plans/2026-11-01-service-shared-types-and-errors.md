# Shared service types and errors consolidation

Approved intent: collect shared public service data/value/request/result types in
one module and all service errors in one error module. Preserve service::... paths,
visibility, field layouts, methods, serde/JSON, diagnostic messages and cause privacy.
Concrete services remain in responsibility-named files; traits remain in mod.rs.

## Plan

1. Gather non-error public values in service/types.rs: model, command DTOs,
   traversal/provenance/status types, search filters, retained trees, configuration
   values/config parsing, and the five data types currently mixed into service/mod.rs.
   Keep type implementations/validation with their fields. No widened fields.
2. Gather all error kinds, error structs, result aliases, validation diagnostics,
   command findings and persisted error/failure records in service/error.rs.
   Preserve distinct error behavior; no generic error wrapper or policy unification.
3. Remove obsolete private files/modules and explicitly reexport all existing public
   names from service/mod.rs. Expose service::types and service::error as the two
   public grouping modules as well. Keep crate-private constants crate-private.
4. Preserve method and test bodies. Rename only colliding private test modules;
   deduplicate byte-identical config-error helpers already shared by two parsers
   in their new owning module. Order private helpers after their callers.
5. Mechanically compare moved items, update guidance, run make fmt, all-target
   compile, independent review and make verify V=1. Check affected Markdown links.
   No new behavioral tests/mutations are needed for declaration/import-only moves.
   Root checks do not imply historical proof, Linux or resource-gate execution.

## Workspace

Base da00dc5. The staged-but-deleted examples/transaction_cost.rs is pre-existing
user state; preserve its index/worktree state. All code changes for this task are
service module organization; no storage/native/build/vendor/dependency changes.
Do not commit, merge or publish without explicit instruction.

## Final result

- `service/types.rs`: 59 public shared data/value/request/response types and pure
  validation, including the five former mod.rs values. `service/error.rs`: 22 public
  error-related types/aliases covering all six error families, diagnostics, command
  findings and persisted failure records. Public root service::... paths preserved.
- Eleven obsolete files removed; concrete services and interfaces remain in place.
- All 145 moved declarations/implementations/tests verified with literal/comment-
  preserving comparison. Only private test module names change; two byte-identical
  config error helpers share one owner. No behavior or field/visibility change.
- All-target cargo check passed. Focused type tests: 15 passed; error tests: 2 passed.
- Independent fresh review: no actionable findings.
- First make verify failed on four subprocess/observer timeouts in cli_process.
  The same suite passed unchanged (5/0); no product/test-deadline changes were made.
  Final `make verify V=1`: exit 0, output `ok`, including Clippy, full debug/release
  root tests and strict Rustdoc. Each profile: 72 root executables, 1013 passed,
  0 failed, 11 ignored; plus 2 compile-fail doctests passed. Nested helper summaries
  are excluded from those totals. The transient failure receipt is retained separately.
- `make fmt`, `git diff --check` and all 909 local Markdown path/anchor checks passed.
- Existing staged/deleted examples/transaction_cost.rs index patch remains byte-identical.
- No changes to storage/native/build/vendor/dependencies/SQL, lib.rs or commands.rs.
  Native runners, Linux and historical proof/resource harnesses were not rerun for
  this declaration-only move; no claims about those suites are made.
- At the verification checkpoint no commits, merges or publishing had been performed;
  base HEAD da00dc5. The user subsequently authorized committing/pushing with `push`.
