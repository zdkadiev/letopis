# Simplify provenance Git dependency

## Intent and agreed scope

The user approved a new trait combining `GitSnapshots` and `SnapshotVerifier`
after discussing unnecessary service generics. Define that application-owned
boundary in `service/mod.rs`, use an owned trait object in `ProvenanceService`,
and remove its Git type parameter and the corresponding historical-cache generic.

Retain the filesystem generic: `FileSystem` declares the `IndexLock` associated
capability. Splitting that interface or erasing its guard is a separate design
change. Preserve existing service APIs, behavior, lock/caching/publication rules,
constructor injection and receiver-owned dependencies.

## Implementation

1. Capture baseline library and ignored-test listings and unrelated staged state.
2. Add documented `VerifiedGitSnapshots: GitSnapshots + SnapshotVerifier`, with
   a blanket implementation for types implementing both original contracts.
3. Store `Box<dyn VerifiedGitSnapshots>` in `ProvenanceService<F>`; simplify its
   implementations and `HistoricalObservations`, preserving their receiver API.
4. Update actual composition-root constructors and test constructors/type names.
   Test recorders retain separate access to their probes; do not add downcasting
   or test-only methods to production contracts.
5. Update current architecture/development guidance; preserve historical docs.

## Verification

- Compare all library test names and ignored statuses against baseline.
- Verify both existing Git implementations and test doubles satisfy the combined
  trait automatically through existing composition and service tests.
- Run relevant provenance unit tests, and mutation-check an adapted test against
  a meaningful Git-contract violation before restoring production code.
- Review service trait visibility/import direction, owned dependency lifetime,
  `lib.rs` wiring, test selectors and unchanged filesystem/transaction contracts.
- Run `make fmt`, `git diff --check`, `make verify`, and changed-doc link checks.
- Independent final review of the uncommitted tree. Native/platform proof runners
  are unnecessary: no adapter, native stack, build or persistence behavior changes.
- Preserve unrelated staged `examples/transaction_cost.rs`; leave changes uncommitted.

## Execution record

- Branch `issue-69`, starting HEAD `b7e167b`; only the unrelated example was dirty.
- Evidence: `/tmp/letopis-verified-git-20261101/`.
- `cargo check --locked --offline --all-targets`: passed.
- `cargo test --locked --offline --lib service::provenance::`: 18 passed.
- Mutation: replacing the strong pin call with lightweight retention made
  `service::provenance::tests::full_audit_still_pins_available_roots` fail its
  independent probe assertions. Exact original source restored; the test passed.
- Independent source/wiring/fixture review: no open findings. Current docs link
  paths and unrelated staged example state checked.

## Final verification

- `make fmt`, `git diff --check`: passed.
- `make verify`: passed, exit 0 with `ok`; formatting, all-target compilation,
  Clippy, debug root tests, strict Rustdoc and release root tests passed on the
  final tree. Log: `/tmp/letopis-verified-git-20261101/make-verify.log`.
- All 463 library test names and 9 ignored names match baseline byte for byte.
- Provenance unit tests: 18 passed; strong-pin mutation failed as expected and
  restored-source exact test passed. No mutation remains in the tree.
- Independent review: no open findings. Actual `lib.rs` constructors inject the
  boxed combined trait; private cache loses its Git generic. Filesystem trait,
  lock types, source-verification behavior and native adapters remain unchanged.
- Changed-document local-link checks passed. Unrelated staged example and its
  absent working-tree file are preserved. Task changes remain uncommitted.
- Native storage/Git platform proof runners were not rerun: no native, build,
  adapter or persistence behavior changed.
