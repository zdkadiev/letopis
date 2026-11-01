# Keep service files owned by service implementations

## Intent

Each responsibility-named file directly under `src/service/` contains a concrete
application service. The established central contracts (`mod.rs`, `types.rs`,
`error.rs`) and the permitted module-local `utils.rs` remain explicit exceptions.
Preserve behavior, public APIs, test names and subprocess selectors.

## Changes

1. Move `is_current_knowledge` from `knowledge_link.rs` into `service/utils.rs`;
   it is shared by search and traversal and uses service-owned interfaces.
2. Move the shared check/status helpers from `check_problems.rs` into the same
   module-local utility file, preserving their rules and errors.
3. Inline `projection.rs` as a private module inside `document_refresher.rs`,
   its sole production owner; keep projection-specific types and algorithms
   there rather than placing them in shared or global utilities.
4. Update imports and module declarations. Retain the service-owned contracts
   and composition root without new services, wrappers or interfaces.
5. Record this ownership rule in current guidance; preserve historical documents.

## Verification

- Compare complete library and ignored-test listings before and after the move.
- Check every service file for its implementation or explicit contract/utility role.
- Check that utility consumers stay within service and projection stays within
  document refresh. Review imports, visibility and `lib.rs` wiring.
- Run `make fmt`, `git diff --check`, `make verify` and changed-document link checks.
- Obtain an independent final review of the actual uncommitted changes.
- No new tests or mutation cycles are needed for an import/module-layout-only
  refactor that preserves existing test bodies. No platform proof runners are
  needed because storage/native/build inputs and behavior are unchanged.
- Preserve unrelated staged `examples/transaction_cost.rs` and leave task changes
  uncommitted for review.

## Execution record

- Starting branch: `issue-69`, HEAD `93e9e83`; only the unrelated example was dirty.
- Evidence directory: `/tmp/letopis-service-owners-20261101/`.

## Added scope: dependencies belong to method receivers

The user additionally requires injected collaborators to be accessed as
`self.dependency`, not forwarded as internal helper-function parameters.

- Convert dependency-using helpers to methods on their owning service or a
  private operation-scoped type with meaningful behavior. Existing contexts
  (SearchEligibility, LinkObservations, historical observations) retain scoped
  borrows rather than adding shared ownership, locks or transaction factories.
- Keep transactions/read views as explicit operation capabilities required by
  existing application trait contracts. They borrow storage for one operation;
  they are not injected long-lived collaborators. Preserve those contracts.
- Shared utility functions take data only. Knowledge matching becomes a pure
  predicate; reading and parsing use the receiver's filesystem and Markdown.
- Check/status diagnostics use an operation-local receiver for shared graph and
  provenance observation rather than forwarding collaborators through helpers.
- Configuration parsing remains pure; provider validation occurs through each
  caller's provider field. No forwarding service or new application interface.
- Convert document-refresh helpers, projection/basis observation, search,
  traversal and historical provenance helpers using disjoint field borrows.
- Preserve behavior, cache and batching bounds, transaction/lock ordering,
  failure causes, lifetime guarantees, test selectors and composition wiring.
- The intermediate verification was interrupted for this expanded scope. Run
  the complete verification on the final combined tree.

- Receiver refactor all-target compilation passed; all 463 library test names and
  9 ignored names still match the baseline. Independent source review found no
  behavioral or ownership issues.
- Initial Clippy caught a rustfmt-expanded assignment missing a semicolon and a
  collapsible configuration-validation conditional. Applied the equivalent
  syntax fixes without lint exceptions; full verification reruns below.

## Final verification

- `make fmt`: passed.
- `cargo check --locked --offline --all-targets`: passed.
- `git diff --check`: passed.
- `make verify`: passed, exit 0 with `ok`, on the final combined ownership and
  receiver-dependency refactor. Includes all-target Clippy, debug root tests,
  strict Rustdoc and release root tests. Log:
  `/tmp/letopis-service-owners-20261101/make-verify-final.log`.
- Final library test listings: 463 names and all 9 ignored names match the
  baseline byte for byte. Existing subprocess selectors are preserved.
- Physical audit: 14 concrete service files plus `mod.rs`, `types.rs`, `error.rs`
  and `utils.rs`; flat directory, private projection owned by document refresh,
  shared knowledge matching is pure, collaborators belong to receiver fields.
- Per-operation transaction/read/write capabilities retain their existing trait
  contracts; `lib.rs` composition is unchanged. No new shared ownership, locks,
  transaction factories or application-owned interfaces were introduced.
- Independent final review and syntax follow-up: no open findings. Existing
  behavior, cache/batch limits and transaction/checkpoint ordering reviewed.
- All 179 local documentation link paths resolve. Unrelated staged example and
  absent working-tree file preserved. Task changes remain uncommitted.
- Native storage/Git platform proof runners were not rerun; build inputs,
  adapters and native behavior were unchanged.
