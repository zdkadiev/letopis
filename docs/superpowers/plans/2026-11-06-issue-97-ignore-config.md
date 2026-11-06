# Discovery Ignore Configuration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement issue #97's root-relative `.letopis/ignore` and
`discovery.respect_gitignore` setting with Gitignore semantics and one consistent
corpus-selection path.

**Architecture:** Application code observes config and ignore-file bytes through
the existing generic filesystem interface. The existing Git implementation
enumerates candidates and matches supplied patterns with pinned `gix`; services
retain Markdown, protected-path, scope and publication policy. Embedding identity
and the storage schema stay unchanged.

**Tech Stack:** Rust 1.95.0, synchronous I/O, existing `gix =0.90.0`,
`gix-dir =0.32.0`, serde_json, existing SQLite/FTS/vec1 and test HTTP fixtures.

**Spec:** [Approved design](../specs/2026-11-06-issue-97-ignore-config-design.md).

Status: approved by the user on 2026-11-06; Native execution is in progress. Work only in
`/Users/zdkadiev/.codex/worktrees/a20c/isssue-97-ignore-config`, branch
`isssue-97-ignore-config`. Base product revision: `52ed491`.

Execution uses the user's recorded Native preference: the primary agent performs
the tasks in sequence, then an independent reviewer checks the complete branch.
Use `superpowers:executing-plans` after approval, plus TDD, Rust guidance and
verification-before-completion when applicable. No implementation is authorized
by approval of the specification alone.

## Global Constraints

- Reuse the existing `gix` library; no new product dependency or lockfile change.
- `.letopis/ignore` follows Gitignore semantics, including excluded parents;
  its pattern base is repository root, not its physical `.letopis/` directory.
- Default `respect_gitignore` is `true`; `false` disables every standard Git
  ignore source, and additional Letopis patterns apply to tracked paths too.
- Missing config defaults; malformed/unsafe/unreadable discovery config stops
  refresh. Valid discovery with invalid embedding settings still permits FTS.
- Git owns enumeration and matching; application services own filenames/config,
  Markdown/protected-path selection and use-case orchestration. Filesystem APIs
  remain generic and domain types stay infrastructure-free.
- Preserve confinement, no-follow/replacement checks, sparse/index behavior,
  raw bytes, deleted tracked membership and synchronous in-process Git.
- Discovery never participates in embedding identity, resets model state or
  adds provider calls to enumeration. Existing orphan cleanup remains valid.
- Recheck observed policy contents/presence before no-op returns and commits;
  author edits do not gain an atomic cross-file snapshot guarantee.
- Use AGENTS.md's naming, visibility, reading order and error conventions.
  Every new/touched test has described Given/When/Then phases and a manual
  mutation check; preserve unrelated edits and historical evidence.
- `make verify` is required on the final changed tree. Record exact checks,
  failures, mutation results and platform limits in fresh evidence directories.
- Commit only task-owned paths; do not merge, push, publish or change GitHub
  issue state without further authorization.

## Review Focus

1. A leaf negation beneath an excluded directory must stay excluded; tracked
   index entries cannot bypass that rule. Pinned by Task 2's ancestor cases.
2. A live policy edit that excludes a newly prepared file must abort publication,
   even though old removal-only rechecks cannot detect it. Pinned by Task 3.
3. Valid discovery alongside broken embedding settings must retain FTS, while
   malformed JSON or unreadable input must preserve the old publication. Pinned
   separately in Tasks 1, 3 and 4.
4. Scoped indexing must not remove outside owners, and an excluded Markdown
   source must remain readable through a surviving external declaration. Task 4.
5. Empty/no-op queries, already-open services, same-stat policy edits, and Git's
   default global ignore path must observe the same policy rules. Tasks 2–4.

## Files and ownership

| Files | Responsibility |
| --- | --- |
| `src/service/types.rs`, `src/service/mod.rs` | Complete config, pure discovery value, Git exclusion mode and application-owned trait |
| `src/service/configuration.rs`, `src/service/init.rs`, `src/service/status.rs`, `src/service/embedding.rs`, `src/cli/output.rs` | Existing config consumers, state interpretation and effective config output |
| `src/git/worktree.rs`, `src/git/repository.rs`, `src/git/mod.rs`; new `src/git/ignore.rs` | Existing enumeration plus private pattern matching; no application file I/O |
| `src/service/utils.rs`, `src/service/document_refresher.rs`, `src/utils.rs` | Generic-file policy observations, candidate filtering and publication rechecks |
| `src/service/inspection.rs` and affected existing test doubles | Adapt changed Git interface without moving responsibilities |
| New `tests/discovery_commands.rs`, `tests/discovery_safety.rs` | Actual-binary corpus lifecycle, all commands and dangerous-file behavior |
| Existing config/embedding/index/Git denial tests and `tests/fixtures/config/corpus.json` | Compatibility, strict validation, vector state and no-subprocess evidence |
| X-1 schema/prose, X-3, decisions, architecture/mechanics, README and Letopis skill guides | Current contracts and author-facing usage |
| New `docs/zkit/letopis/checks/issue-97/report.md`; fresh evidence below that directory or ignored `.artifacts/issue-97/` | Final traceability and validation receipts |

No new filesystem trait, service implementation, runtime, persistent policy
cache, storage query or schema is planned. Review `src/lib.rs`'s real constructor
wiring; its existing collaborators already provide what this feature needs.

---

### Task 1: Model and expose complete discovery configuration

**Files:** Modify `src/service/types.rs`, `src/service/mod.rs`,
`src/service/configuration.rs`, `src/service/init.rs`, `src/service/status.rs`,
`src/service/embedding.rs`, `src/cli/output.rs`, `tests/config_command.rs`,
`tests/init_command.rs`, `tests/fixtures/config/corpus.json`,
`docs/zkit/letopis/contracts/schemas/X-1-cli.yaml` and affected embedding tests.

**Interfaces:**

- Preserve `Configuration::parse(&[u8]) -> EmbeddingResult<Configuration>`,
  `get_enabled() -> bool`, `get_max_retries() -> u64`, and
  `get_embedding_config() -> Option<&EmbeddingConfig>`.
- Change `Configuration` into a complete value with private embedding-state
  and discovery fields. Keep the current finite embedding alternatives in a
  private `EmbeddingState` enum in `service/types.rs`.
- Add public `DiscoveryConfig: Debug + Clone + Copy + PartialEq + Eq + Default`,
  with private `respect_gitignore: bool`, `parse(&[u8]) -> EmbeddingResult<Self>`,
  and `get_respect_gitignore(&self) -> bool`.
- Add `Configuration::get_discovery(&self) -> DiscoveryConfig`; reexport the
  new public value from `service/mod.rs`.
- Add `Configuration::into_embedding_config(self) -> Option<EmbeddingConfig>`
  for existing consumers that must take ownership of parsed settings; this
  avoids cloning the provider parameter map when replacing enum pattern matches.
- A private `parse_config_root(&[u8]) -> EmbeddingResult<Map<String, Value>>`
  validates unique-key JSON, object shape and allowed root keys. Shared
  `parse_discovery_section(&Map<String, Value>) -> EmbeddingResult<DiscoveryConfig>`
  owns discovery validation. Full parsing additionally requires/validates
  `embedding_model`; discovery-only parsing does not validate that section.

- [x] **Step 1: Add failing pure config and CLI contract tests.** Name the new
  unit tests `discovery_defaults_preserve_embedding_identity`,
  `discovery_projection_ignores_invalid_embedding_settings`, and
  `discovery_rejects_ambiguous_or_invalid_input`. Extend the shared corpus with
  absent section, `{}`, true/false, nulls, string/number/array values, duplicate
  root/nested keys and unknown keys. Discovery-only parsing accepts a known-key
  object with missing/invalid embedding state; full parsing rejects it.

  Pin both toggle values to this independently computed SHA-256 of the literal
  canonical four-field embedding fixture, not to one another:

  ```rust
  assert_eq!(configuration.get_embedding_config().unwrap()
      .get_identity().to_hex(),
      "c713040f35c46c21c7389aa69b25910ad51fb1ce6ceb26b378513ea5dec8748b");
  assert!(!configuration.get_discovery().get_respect_gitignore());
  ```

  Fixture fields: provider `ollama`, model `fixture`,
  parameters `{"base_url":"http://127.0.0.1:9/api"}`, max_retries `2`.
  Keep enabled/disabled identity coverage. Add literal CLI output assertions for
  effective discovery defaults and false. Assert `init` preserves old valid bytes,
  adds defaults only to a newly generated config and does not create `ignore`.
- [x] **Step 2: Confirm red tests.** Run
  `cargo test --locked --offline --lib discovery` and
  `cargo test --locked --offline --test config_command --test init_command`.
  First failures must identify missing discovery support or old literal output.
- [x] **Step 3: Implement the value/parser/output contracts above.** Reuse
  `JsonValue<true>` and existing embedding projection/JCS logic. Retain migration
  diagnostics. Adapt `Configuration::...` matches throughout current source/tests
  to the accessor/consuming API; enabled execution checks the enabled flag and
  consumes its owned `EmbeddingConfig`. Do not expose the
  private enum merely to reproduce cross-module pattern matching.
- [x] **Step 4: Update schema and generated/expected config literals.** Input
  `discovery` and its boolean member are optional; unknown members are rejected.
  Output includes effective defaults after `embedding_model`. Preserve missing
  config's null output and strict validation without provider HTTP.
- [x] **Step 5: Run focused tests and schema agreement.** Repeat Step 2, then
  `cargo test --locked --offline --test embedding_service --test status_command`.
  Use a Python environment with `jsonschema==4.26.0` and PyYAML to run
  `python -B tests/config_schema_agreement.py`; expect exit 0 and `errors: []`.
  System Python currently lacks both packages. Reuse a suitable prepared env or
  prepare an ignored task env during execution and record package versions;
  do not alter Cargo dependencies or another task's environment.
- [x] **Step 6: Mutation-check defaults, validation and identity tests.** Flip
  the absent-section default, accept a wrong-type field, then include discovery
  in identity, one mutation at a time. Each named test must fail. Restore exact
  source and rerun those tests; record outcomes before committing.
- [x] **Step 7: Commit the config deliverable.** Stage only this task's actual
  paths, inspect the staged diff, commit `feat: add discovery configuration`.

### Task 2: Enumerate and filter Git paths with supplied ignore rules

**Files:** Modify `src/service/types.rs`, `src/service/mod.rs`,
`src/git/worktree.rs`, `src/git/repository.rs`, `src/git/mod.rs`,
`src/service/utils.rs`, and affected Git mocks in
`src/service/document_refresher.rs`/`src/service/inspection.rs`.
Create `src/git/ignore.rs` with inline unit tests.

**Interfaces:**

- Add/reexport `GitIgnoreMode::{Standard, Disabled}` in service types.
- Change the application trait and its implementations together:
  `fn worktree_paths_get(&self, ignore_mode: GitIgnoreMode, patterns: &[u8]) -> GitResult<Vec<u8>>`.
- Private enumeration helper:
  `worktree_paths_get(git: &GitRepository, root: &Path, ignore_mode: GitIgnoreMode, patterns: &[u8]) -> GitResult<Vec<u8>>`.
- Private matching entry point in `git/ignore.rs`:
  `paths_filter_by_patterns(raw: &[u8], patterns: &[u8], case: gix::glob::pattern::Case) -> GitResult<Vec<u8>>`.
  It owns no filesystem or application configuration dependency.

- [x] **Step 1: Add failing literal and Git-oracle tests.** In
  `git::ignore::tests`, pin at least these cases with explicit expected results:

  | Patterns | Excluded | Included |
  | --- | --- | --- |
  | `/root.md` | `root.md` | `sub/root.md` |
  | `draft.md` | `draft.md`, `sub/draft.md` | `draft.markdown` |
  | `notes/` then `!notes/keep.md` | `notes/keep.md` | `other/keep.md` |
  | `notes/*` then `!notes/keep.md` | `notes/drop.md` | `notes/keep.md` |
  | `notes/**` then `!notes/deep/` then `!notes/deep/keep.md` | `notes/deep/drop.md` | `notes/deep/keep.md` |
  | `**/generated?.[mM][dD]` | `a/generated1.md` | `a/generated11.md` |

  Add ordering in both directions, re-excluded parents, directory-versus-file,
  `*`/`?` slash boundaries, zero/multiple `**` levels, escaped `#`/`!`/`*`, escaped
  trailing spaces, empty/comment lines, CRLF, malformed brackets, dangling escape,
  Unicode, byte filenames and literal `$` (no precious extension).

  In `git/repository.rs`'s inline tests, enumerate SHA-1 and SHA-256 fixtures
  against independent `git ls-files -z --cached --others` with/without
  `--exclude-standard`. Pin tracked ignored/deleted/staged paths, conflict-stage
  deduplication downstream, sparse membership, nested ignores, `info/exclude`,
  explicit/default global excludes and `core.ignoreCase` true/false. Capture
  user HEAD/index/config/ref state before product calls and require preservation.

  Pattern differential oracle: use a separate scratch repository with the same
  patterns at root `.gitignore`, feed raw NUL names to
  `git check-ignore --no-index -z --stdin`, and compare independently returned
  ignored names. Accept oracle exit 0 or 1, reject other exits. Never make the
  product's own matcher compute the expectation. Isolate HOME/XDG and user Git
  settings per child command without process-global environment mutation.
- [x] **Step 2: Confirm red tests.** Run
  `cargo test --locked --offline --lib git::ignore::tests` and
  `cargo test --locked --offline --lib git::repository::tests`.
  Missing APIs or differing membership are the expected initial failures.
- [x] **Step 3: Implement enumeration and matching.** Preserve index expansion;
  build the standard exclusion stack only in Standard mode. Disabled mode passes
  no exclusion stack to the existing `gix_dir` walk; do not replace the walker.
  Apply supplied patterns after combining raw index and untracked membership.
  Match each ancestor directory from root toward leaf and stop on an effective
  positive exclusion; otherwise use the leaf's last matching pattern. Use
  `gix::ignore::Search`, Git-compatible parsing and repository case behavior.
  An ignore buffer's physical filename must not change its root pattern base.
- [x] **Step 4: Adapt all existing trait callers and test doubles.** Until Task
  3 connects application settings, CandidateSelection supplies Standard and an
  empty pattern slice, preserving old product behavior. Adjust mock argument
  assertions and test callbacks. Preserve the existing cancellation observer's
  `worktree_paths_get` symbol and the no-product-subprocess guarantees.
- [x] **Step 5: Run the new tests and existing corpus/safety regressions.**
  Repeat Step 2, then
  `cargo test --locked --offline --lib service::document_refresher::corpus` and
  `cargo test --locked --offline --test git_no_subprocess --test git_snapshots`.
  Every differential case must agree with Git and its literal expected cases.
- [x] **Step 6: Mutation-check parent precedence, raw-path filtering and toggle
  scope.** Bypass ancestor matching, reverse pattern order, and always construct
  standard excludes, independently. Named tests must detect each mutation.
  Restore all changes and rerun affected cases.
- [x] **Step 7: Commit `feat: support Git discovery exclusion policies`.**
  Review the staged file list and confirm no dependency/native-stack changes.

### Task 3: Observe policy inputs and preserve refresh consistency

**Files:** Modify `src/service/utils.rs`, `src/service/document_refresher.rs`,
`src/utils.rs`, and candidate-observation mocks in `src/service/inspection.rs`.
No new infrastructure component or injected dependency is required.

**Interfaces:**

- Add application-owned `IGNORE_PATH = ".letopis/ignore"` beside `CONFIG_PATH`.
- In service utils add private operation results:
  `DiscoveryObservation { config_bytes: Option<Vec<u8>>, ignore_bytes: Option<Vec<u8>> }`
  and `CandidateObservation { paths: Vec<RepoPath>, discovery: DiscoveryObservation }`.
  Give only the owning service module necessary visibility; they are observations,
  not new constructors/services or permanent policy caches.
- `CandidateSelection::get_candidates(&self, paths: &[RepoPath]) -> DocumentResult<CandidateObservation>`.
- `CandidateSelection::get_discovery(&self) -> DocumentResult<DiscoveryObservation>`
  safely observes both files without parsing. Candidate collection parses the
  config projection with Task 1 and supplies Task 2's mode/patterns.
- `CandidateSelection::verify_discovery(&self, previous: &DiscoveryObservation) -> DocumentResult<()>`
  reobserves raw contents/presence and rejects differences. All I/O goes through
  `self.filesystem`; pure comparison never receives injected collaborators.
- `DocumentRefreshService::get_candidates` returns CandidateObservation; Delta
  carries the consumed discovery observation until a no-op return or commit.

- [x] **Step 1: Add failing service tests.** Add an inline `tests::discovery`
  module in `document_refresher.rs` for selection and publication. Pin tracked
  exclusion, ignored-untracked inclusion, later restore, changed policy through
  the same service/shared Git handle, same-stat byte edits, comment-only policy
  edits with no revision bump, and full mode's existing bump behavior.
  Adapt `tests/knowledge_search.rs`'s old malformed/unreadable fallback regression
  into named scenarios: missing config succeeds; malformed JSON/unreadable input
  fails and preserves the publication; invalid embedding with valid discovery
  still returns the literal expected FTS IDs.
- [x] **Step 2: Add failing deterministic race tests.** Add an inline
  `tests::process::discovery` child module beside the existing process scenarios
  so it can reuse its parent's private socket/checkpoint fixtures and state
  snapshots; do not expose fixtures to sibling production modules. No sleep-based race
  assertions. Add `discovery_edit_before_lock_is_redetected`,
  `discovery_edit_during_preparation_rolls_back`,
  `discovery_edit_before_commit_rolls_back`, and
  `discovery_edit_before_noop_return_is_rejected`. Exercise config and ignore
  files, presence changes and both membership directions. Assert exact old
  nodes/edges/files/FTS/revision and retained embedding state on abort.
- [x] **Step 3: Confirm red tests.** Run
  `cargo test --locked --offline --lib discovery`
  and `cargo test --locked --offline --test knowledge_search`.
- [x] **Step 4: Implement observation and selection.** Check the private
  directory through existing generic capabilities, read each file with existing
  no-follow/identity-safe methods and bounded Changed retry, parse discovery,
  pass generic Git arguments and preserve application filtering order. Missing
  files differ from every typed file failure. Invalid config maps to existing
  InvalidRequest with its private cause; observation errors remain InternalError.
  Adapt touched mocks by path so new policy reads do not weaken document-read
  counts or existing failure/race assertions.
- [x] **Step 5: Implement final rechecks.** Keep preliminary redetection and
  locked publication independent. Recheck Delta's policy on preliminary no-op,
  locked no-op after snapshot validation, and after prepared updates/removals
  immediately before commit. The existing `before_commit` test checkpoint must
  run before the final policy recheck. Add a test-only no-op checkpoint if needed
  to prove its schedule. Raw contents/presence changes return the existing
  documents-changed error; unsafe final observations retain file failures.
  Keep `verify_excluded_candidates`; it still protects candidate reappearance.
  Its own fresh listing must not replace comparison with the original Delta
  observation, since new exclusions also matter.
- [x] **Step 6: Run focused regression tests.** Repeat Step 3, then
  `cargo test --locked --offline --test document_refresh --test check_lifecycle --test index_command --test knowledge_inspection`.
  Run `cargo test --locked --offline --lib service::document_refresher::tests::process`;
  expected all pass with no ignored
  parent schedule substituted for real execution.
- [x] **Step 7: Mutation-check each final check independently.** Remove the
  preliminary no-op check, locked no-op check, final commit check and locked
  policy reread one at a time. The corresponding schedule must fail while the
  others can remain intact. Restore, rerun and commit
  `feat: apply discovery policy consistently during refresh`.

### Task 4: Verify every command, safety boundary and embedding lifecycle

**Files:** Create `tests/discovery_commands.rs`, `tests/discovery_safety.rs`;
modify `tests/embedding_service.rs`, `tests/git_no_subprocess.rs`,
`tests/config_command.rs`, `tests/init_command.rs` and `tests/index_command.rs`
where their owned scenarios need extension. Reuse `tests/support` fixtures;
extract shared helpers only if two real consumers require them.

**Interfaces:** Existing actual-binary `ReadingRepo::invoke(&[&str], &[u8]) -> Output`,
`invoke_in(&Path, &[&str], &[u8]) -> Output`, private test helpers, storage
inspection scopes and the synthetic `HttpFixture`. No new production API.

- [ ] **Step 1: Write the actual-command matrix as failing tests.** Each
  refreshing command receives a fresh independently prepared repository so a
  preceding command cannot hide a missing refresh. Use tracked `keep.md` and
  `vendor/drop.md`, plus Git-ignored untracked `notes/local.md`, with distinct
  literal keywords and known element IDs. Publish baseline, change only policy,
  invoke the command and inspect the result plus cache membership.

  | Command | Required outcome |
  | --- | --- |
  | `check` | Same membership; clean result for valid surviving graph; separate remaining-owner dangling-link scenario reports excluded target |
  | `search` | Excluded IDs absent, ignored local ID present only with toggle off; empty query still refreshes safely |
  | `traverse` | Excluded start returns `not_found`; outgoing/incoming references follow existing graph rules |
  | `inspect` | Excluded requested knowledge makes mixed batch fail; surviving capture omits excluded owners; external source binding still captures its exact bytes |
  | `index`, `--verify`, `--full`, both combined orders | Exact files_updated/files_removed/published/revision; scoped remove preserves outside owner; full retains existing forced-publication behavior |
  | `config` | Effective discovery value/default, no publication, no provider call |
  | `init` | Explicit generated defaults, preserve existing bytes/data, no ignore-file creation or corpus publication |
  | `status` | Observe old publication until refresh, no policy-induced publication; invalid config problem retains existing classification |
  | `embed --drain` | Published work controls membership until refresh; after refresh removed owners are not new work; invalid config blocks provider execution under existing drain behavior |
  | Help/version | Global help, all public command helps and version stay independent of config/ignore files, repository and stdin |

  Run representative policy/error scenarios from a nested directory. Include
  `.md` and `.markdown`, invalid markup that becomes excluded, deleted tracked
  paths and hard-protected nested `.git`/`.letopis` components with negations.
- [ ] **Step 2: Write dangerous-input tests.** In discovery_safety, use owned
  child watchdogs for ignore FIFO/directory/symlink cases and unsafe parents;
  assert bounded failure and unchanged logical index. Also cover missing/empty
  ignore, replacement during observation, malformed config and literal safe
  error envelopes with no sentinel paths/content. Test permission denial through
  an injected file failure and a real file where the host enforces it; record any
  privileged-host limitation. Stable final document symlinks/nonregular files
  remain silently absent; unsafe parent traversal remains fatal.
- [ ] **Step 3: Confirm failures and strengthen state-preservation tests.** Run
  `cargo test --locked --offline --test discovery_commands --test discovery_safety`.
  Add `discovery_only_changes_preserve_embedding_state` in embedding_service:
  seed literal vectors, attempts and query error, change only discovery, refresh
  through the library, and compare prior identity and relevant rows exactly.
  Check both no-membership-change and removal of one of two shared-hash owners;
  retained hashes keep their vector and attempt state. Assert zero HTTP for
  enumeration and config/status/init. Allow legitimate inference/drain effects
  only in separately identified command scenarios.
- [ ] **Step 4: Extend helper-denial coverage.** Run the configured true/false
  modes and nonempty additional patterns through the existing poisoned-PATH
  product runner. Fixture Git/oracles run outside denial intervals. Preserve
  SHA-1/SHA-256 and the Linux process-trace assertion; no shell/helper fallback.
- [ ] **Step 5: Make only missing feature corrections in their existing owners.**
  Test helpers stay private with scenario assertions visible. Repeat Step 3 and
  `cargo test --locked --offline --test embedding_service --test git_no_subprocess --test config_command --test init_command --test index_command --test cli_help`.
- [ ] **Step 6: Mutation-check the matrix.** Independently bypass additional
  pattern filtering, omit Disabled mode, include discovery in embedding identity,
  ignore an unsafe policy read and drop scoped-removal confinement. Each targeted
  scenario must fail. Restore exact source and rerun the corresponding tests.
- [ ] **Step 7: Commit `test: cover discovery policies across commands and races`.**
  Include any focused production fixes in the same task-owned commit and report
  why they were needed; do not create tests that merely mirror the implementation.

### Task 5: Reconcile documentation and run final acceptance

**Files:** Modify `README.md`, `docs/zkit/letopis/contracts/X-1-cli.md`,
`docs/zkit/letopis/contracts/X-3-git.md`, `docs/zkit/letopis/decisions.md`,
`docs/zkit/letopis/architecture.md`, `docs/zkit/letopis/mechanics.md`,
`skills/letopis/SKILL.md`, `skills/letopis/references/operations.md` and any current
source-linked concept whose discovery/config claim actually changes.
Create `docs/zkit/letopis/checks/issue-97/report.md`; update this plan's checkboxes
and the approved spec's implementation status without rewriting historical proof.

**Interfaces:** The final public config/schema, Git and refresher interfaces
from Tasks 1–4. No new product mechanism is introduced by this acceptance task.

- [ ] **Step 1: Update current contracts and usage.** Explain true/false source
  scope, tracked filtering, root-relative examples, parent negation, effective
  defaults, absent-ignore init and failure behavior. Update D-97/D-104 with dated
  clarifications and consistent architecture/mechanics ownership. Preserve
  historical decisions and captured examples in proof harnesses. Use the Letopis
  skill for any affected marked knowledge/source_snapshot updates; do not simply
  change an OID without checking its addressed source bytes.
- [ ] **Step 2: Self-review architecture and requirement coverage.** Inspect
  physical files, visibility/import direction, trait implementations, private
  helper ownership, actual lib.rs constructor wiring and full config-versus-
  embedding identity separation. Trace every spec matrix row to an exact test
  name or receipt. Resolve gaps before the broad run.
- [ ] **Step 3: Create a fresh evidence directory and run host acceptance.**
  Inventory changed source hashes, tool versions, base/HEAD/diff, case names and
  commands. Run `make fmt`, then `git diff --check`, then `make verify` with its
  full stdout/stderr captured. Require final exit 0 and `ok`; never infer success
  from an ongoing process. Debug/release tests and strict Rustdoc must all pass.
  Run schema agreement with the prepared interpreter and affected Markdown link
  checks. Record ignored helpers/real-provider/corpus tests accurately. Do not
  rerun make verify on an unchanged tree just to get another identical receipt.
- [ ] **Step 4: Run focused Linux evidence using available Docker.** Build the
  existing `tests/git/Dockerfile` from this changed source as a task-specific
  local image. Record image ID, architecture and Git/Rust versions. Run current
  Rust targets offline with a mounted fresh receipt directory:
  `cargo test --locked --offline --lib git::`,
  `cargo test --locked --offline --lib discovery`,
  and `cargo test --locked --offline --test discovery_commands --test discovery_safety --test git_no_subprocess`.
  Set `LETOPIS_TEST_STRACE=/usr/bin/strace` and
  `LETOPIS_TEST_EVIDENCE=/receipt` for the denial suite, under `--network none`.
  Use owned bounded containers and preserve logs before cleanup. No obsolete
  CLI transport runner or unrelated historical proof is silently substituted.
  If infrastructure is unavailable, retain the concrete failure and label Linux
  evidence unrun; do not weaken assertions or claim platform completion.
- [ ] **Step 5: Get one independent final branch review.** Under Native
  execution, use the requesting-code-review skill and a fresh reviewer on the
  most capable available model. Review base `52ed491` through the actual changed
  tree against the approved spec/plan, prioritizing parent rules, races, typed
  failures, all commands, embedding preservation and boundaries. Treat review
  findings as hypotheses to verify, fix confirmed issues, mutation-check affected
  tests, and repeat only checks invalidated by those changes. A Rust source,
  Rust API documentation or dependency correction after the acceptance run
  requires `make verify` again on the corrected tree; Markdown-only corrections
  require their affected link/consistency checks.
- [ ] **Step 6: Finalize the report and commit intended deliverables.** Include
  a row for every acceptance area, exact passing commands and mutation failures,
  source-hash/evidence references, reviewer outcome and platform limitations.
  Summarize synthetic HTTP fixtures honestly. Commit documentation/evidence
  paths only after reviewing staged content for payloads and accidental generated
  state. Finish with clean task worktree status and report the branch, commits,
  tests and remaining limits. Leave integration and GitHub actions for explicit
  user authorization.

## Plan self-review

Spec coverage: configuration and failure policy → Tasks 1/3/4; Git semantics and
raw selection → Tasks 2/3; boundaries → Tasks 1–3/5; publication and commands →
Tasks 3/4; full verification and current documentation → Task 5. All five Review
Focus items have named owning tasks. Signatures are defined before consumers;
Task 2 supplies legacy defaults only until Task 3 wires policy observations.

This plan preserves the already completed baseline (`cargo test --locked
--offline`, exit 0) as baseline evidence only. No task checkbox is complete yet;
plan preparation did not implement the feature or run its proposed tests.
