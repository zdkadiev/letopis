# Focused AGENTS.md consistency cleanup

Approved implementation plan supplied by the user on 2026-11-01.

## Scope and constraints

Align guidance and affected code with agreed conventions. Preserve runtime behavior,
persisted schemas and CLI/JSON contracts. No compatibility aliases or migrations.
Keep historical proof sources and compatibility fixtures unchanged. Do not migrate
phase comments repository-wide. Do not commit or publish without authorization.

## Task 1: Guidance

Make AGENTS.md canonical for coding conventions. Synchronize development and
architecture guidance: external/standard trait naming exception; owned application
traits renamed together; caller order applies to owned implementation code, with
tests-last and recursive-cycle exceptions; historical SQL fixtures remain intact.
Describe implemented CLI/TK-9 layout in present tense. Markdown-only changes need
link/consistency checks; Rust/API-doc/dependency changes require make verify.
Require descriptive phases in new/modified tests, retaining untouched-test backlog.

## Task 2: Focused code changes

- Replace vector schema selector booleans with private VectorSchema::{Main, Attached}.
- Replace test process helper selector with ProcessGroup::{Inherited, Owned}.
- Rename resolve_node_by_id to get_node_ref_by_id, match_text to
  get_text_matches_by_expression, find_nearest_vectors to get_nearest_vectors,
  across active interfaces, implementations, callers and wrappers.
- Use search_hash for corresponding vector storage parameters and error for stored
  sanitized diagnostic parameters. Preserve caller sanitization requirements.
- Replace Definition tuple with private SchemaDefinition { object_type, name, sql }
  deriving Debug, PartialEq, Eq. Preserve normalization and comparisons.
- Correct phase descriptions in tests modified by this work. Retain configuration
  booleans, observed state, predicates, results, const generics and external signatures.

## Task 3: Verification

Cover both schema locations, identity and dimensions, process groups, FTS ordering,
nearest-vector results, node resolution, failure propagation, uppercase legacy schema
acceptance and case-sensitive literals. Mutation-check new behavioral tests and restore
sources byte-for-byte. Review module boundaries, visibility, interfaces and wiring.
Run make fmt, git diff --check and host storage runner in a new evidence directory
(the runner includes make verify). Check affected Markdown links and consistency.
Report Linux as unrun unless executed; record untouched convention backlog separately.

## Execution record

Changes remain in the clean-at-start `issue-69` worktree. No commit, push, merge,
issue update, persisted-schema change or wire-format change is part of this work.

- Storage owns `VectorSchema` and `SchemaDefinition`; neither escapes storage.
  Application traits and all active forwarding implementations/callers use the
  renamed retrieval methods. The standalone storage example is an active caller.
- `ProcessGroup` is private test support. Domain, service dependencies, adapter
  boundaries and `lib.rs` composition remain unchanged.
- Existing coverage retains FTS/native-vector ordering, node resolution, identity
  rejection, rollback/failure propagation, legacy uppercase schema acceptance and
  quoted-literal case sensitivity.
- New selector tests passed. Three compiling mutations redirected attached identity,
  attached dimensions, and process-group selection; each failed its independent
  assertion. The mutation script restored and SHA-256-checked original source bytes.
- Fresh whole-change review found no runtime or architecture issues. Four test-phase
  placement findings were corrected. No review finding remains deferred.
- Initial host evidence `host-20261101-1` records a failed all-target check caused by
  two missed example API calls. Those calls were renamed; a separate second evidence
  directory retains the subsequent run.

### Execution rulings

1. Retain the existing clean task worktree, following the plan's execution assumptions.
   If that interpretation were wrong, uncommitted changes would need relocation.
2. Keep existing behavioral regression coverage for spelling/type refactors and add
   focused selector tests with semantic mutations. If insufficient, a regression
   outside that coverage could remain; the full host gate and review supplement it.
3. Treat the review's phase-placement findings as acceptance fixes because this plan
   explicitly requires accurate phases in modified tests. These are comment-only
   corrections; the cost is additional comment diff, with no behavior change.

### Untouched backlog and limits

The planning scan counted 3,029 phase markers without descriptions, not independently
reviewed tests. This change updates phases in modified tests only; untouched active
tests remain a retrofit backlog. No repository-wide convention compliance is claimed.
Linux and historical proof harnesses are not rerun by this focused host verification.


## User-requested transaction naming extension

The user additionally requested `tx` / `begin_tx` names for functions returning
transactions. Use `begin_read_tx`, `begin_cache_write_tx`, `begin_vector_write_tx`
and `begin_status_read_tx` on the storage interfaces; use `begin_tx` on concrete
transaction types, with `begin_optional_tx` / `begin_available_tx` for the read
variants. Rename all active callers and forwarding implementations, including
function references passed to `map`. Keep external rusqlite signatures unchanged.
No transaction type, lifetime, behavior or commit/rollback contract changes.

The second host run was deliberately interrupted when this scope was added; its
partial debug successes are not final acceptance. A fresh evidence directory is
required for the final source tree. The API-only rename touches additional tests;
apply the existing phase-description rule to those touched scenarios too.

The naming question received no answer during the offered clarification window;
the stated recommended convention above was used. The all-target check passed
with these names. Additional review confirmed transaction boundaries, lifetimes,
forwarding and external method names remain unchanged. Inaccurate sequential-test
phase descriptions and non-colon marker variants were corrected. The final scan
found no incomplete phase markers in changed Rust functions.

A final comment audit corrected the sequencing description in the independent
cache/vector writer test. The third host run was interrupted during early checks;
`host-20261101-4` is the final evidence directory. Interrupted runs are not passes.

`host-20261101-4` failed on four subprocess startup deadlines in the debug library
suite. All four cases passed individually; the unchanged default-parallel library
suite then passed (454 passed, zero failed, nine ignored, 16.16 seconds). Host load
was 14.80 on 14 logical CPUs; transient contention is a plausible explanation, not
a proven cause. No test deadline or product source changed during diagnosis.
The final host run is `host-20261101-5`, using documented Python 3.14.7 from
`/opt/homebrew/bin/python3`, with default test parallelism.

## Final verification — PASS

- `make fmt` and `git diff --check`: passed.
- `/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/agents-consistency/host-20261101-5`: exit 0, manifest `PASS`; all 16 recorded commands exited 0.
- Runner-owned `make verify V=1`: passed all-target checks, Clippy, strict Rustdoc,
  and root debug/release suites. Each profile: **1,010 passed, zero failed,
  11 ignored** across 72 root executables plus doc-tests. Eight nested helper
  summaries per profile are excluded from these totals.
- Real 60-second provider deadline: one passed test, 60.01 seconds.
- Canonical DDL parity, runner tests, native SQLite/vec1 capabilities, copied
  standalone executable, exact vector bytes/reopen/revision/FTS and dynamic linkage:
  passed. The standalone receipt reports revision 1.
- All **457 captured source-input SHA-256 hashes** match the final workspace.
- Three semantic selector mutations were rejected; source bytes were restored.
- All **178 local Markdown paths and anchors** in affected guidance resolve.
- No old retrieval or transaction method names, `own_group`, or `attached: bool`
  remain in active Rust under `src/`, `tests/` and `examples/`. Changed-function
  scans found no incomplete phase markers.
- No SQL fixture, dependency, `lib.rs` wiring or command-wiring changes. Historical
  proof sources and compatibility fixtures retain their original contents.
- Linux and separate historical proof harnesses were not run. Untouched convention
  findings remain a backlog; no repository-wide compliance claim is made.

Work remains uncommitted on `issue-69` at base `96f39bb`; no push or merge occurred.
Failed/interrupted receipts remain separate from the passing final receipt.
