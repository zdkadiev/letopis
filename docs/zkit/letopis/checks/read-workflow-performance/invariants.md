# Read optimization — invariant audit

Date: 2026-10-29. Status: implementation and regression/mutation checks are present; final whole-tree/native gates passed. Historical design-audit requirements below are retained as the original checklist; see report.md for execution results. Scope: [revised spec](../../../../superpowers/specs/2026-10-29-read-workflow-performance-design.md) and [plan](../../../../superpowers/plans/2026-10-29-read-workflow-performance.md).

Evidence labels: **current** means inspected in the current product or existing tests; **required** names a test that must be implemented/run for the revised behavior. Neither static review nor passing the old tests establishes that new behavior works. Physical object-store damage concurrent with the check cannot be ruled out indefinitely by any one verification pass.

## Freshness, audit scope and publication

| ID | Invariant / condition | Current evidence and required proof |
| --- | --- | --- |
| I-01 | Reads update the database from live Markdown without writing authored files | Current: read service refresh calls, document projection code. Required: additions/edits/deletions remain visible; compare canonical bytes and user Git state before/after. |
| I-02 | Routine source retention never performs a full source-closure walk, including first use of a new basis and a Markdown edit | Required: process-count/spy tests distinguish root/ref work from full audit and inspect's own reading snapshot; scale unrelated source objects. This is a changed guarantee. |
| I-03 | Omitted/empty check.paths selects full verification; nonempty selects scoped findings and routine retention | Required: CheckService recording tests for omitted, empty, one path, and a nonempty list naming every current file; CLI decoding coverage. No hidden dependence on file-count equality. |
| I-04 | Full check audits without reparsing everything; reindex reparses and audits | Required: separate publication mode/verification scope tests. Full incremental/no-delta check still verifies; it must not become an alias for reindex. |
| I-05 | Full audits run under index.lock before any new SQL write/commit; failure preserves the previous graph/FTS revision | Current: basis-preflight order and rollback tests. Required: revised verified-refresh tests with newly edited knowledge plus an old corrupt basis; no BEGIN/commit after audit failure. Existing refs may remain, without implying verification success. |
| I-06 | Ordinary refresh retains available roots before SQL publication; root/ref failure prevents publication | Current: original strong-retention ordering. Required: same ordering with lightweight retention, including absent-to-available bases and no-change calls. Do not drop retention merely to enable a fast return. |
| I-07 | Audited roots belong to the resulting valid projection | Required: unchanged valid elements and last-valid elements of invalid Markdown keep roots; valid replacement/removal removes obsolete roots. Deleting the last knowledge claim for an obsolete damaged basis must not require auditing unrelated history. Compute metadata before SQL, not after publication. |
| I-08 | Whole-check findings refer to the exact audited database revision | Found during audit: refresh releases its lock before CheckService opens a read view. Required: compare verified RefreshOutcome.revision with pinned read revision; reject a newer unaudited revision safely. A later publication after pin must leave the old consistent view usable. No automatic unbounded retry. |
| I-09 | Captured files changing during preparation do not create mixed projections | Current: identity/stat/hash checks and fail_changed paths. Required: rerun relevant process schedules for both verified incremental refresh and reindex; preserve pre-commit checkpoints. SQLite consistency is not an atomic snapshot of arbitrary concurrent live file edits. |
| I-10 | Source-only edits do not silently confirm author knowledge | Current: source provenance compares addressed bytes; source_snapshot is author metadata. Required: source-only edit updates flags as appropriate but never changes created_at/source_snapshot automatically. |

## Retention and object integrity

| ID | Invariant / condition | Current evidence and required proof |
| --- | --- | --- |
| I-11 | Lightweight retention validates the exact tree root/ref without claiming validated descendants | Required: retain returns Result<()> and cannot manufacture RetainedTree; missing/wrong-format/wrong-type roots have documented errors or author-basis diagnostics. Strong pin remains a separate capability. |
| I-12 | Every strong pin/full audit detects incomplete or corrupt closures, including existing refs and reopened handles | Current: git_corruption and git_retention tests. Required: keep them green through batching; existing refs must never bypass strong verification. |
| I-13 | Reads detect corruption of the root and objects actually used; unrelated damage is deferred to full audit | Required: damage unrelated descendant, verify normal reads skip it and full check/reindex fail; separately corrupt addressed source/tree/blob and verify safe failure. Never describe a routine successful read as proof that all descendants are healthy. |
| I-14 | Direct refs protect healthy reachable objects against ordinary GC; ref conflicts never overwrite user state | Current: exact-ref checks, CAS, symbolic/conflicting ref rejection, concurrent pin and GC/reopen tests. Required: equivalent lightweight-retain tests; preserve HEAD/index/branches/user refs and never signal unrelated writers. |
| I-15 | Missing/invalid author bases remain diagnostics; missing requested IDs and corrupted required children retain distinct classifications | Current: SnapshotVerifier, provenance and inspection error mapping. Required: matrix of author versus requested root, wrong type/format, missing child, malformed bytes and permission/I/O failures in both verification modes. Do not convert all failures to not_found. |
| I-16 | Hash verification covers exact canonical Git bytes in SHA-1 and SHA-256, including collision rejection policy | Required: independent Git/literal vectors, wrong-OID decodable-object fixtures, empty bodies and binary data. Dependency/API probe for collision-detecting SHA-1 must pass first. No unchecked SHA-1 substitution. |
| I-17 | Tree traversal matches existing Git mode/name/OID semantics, deduplicates objects and skips gitlinks | Current: Git ls-tree parsing and visited type map. Required: raw-mode normalization oracle including100664/100600, executable/symlink/directory/gitlink fixtures, malformed/truncated entries and conflicting expected types in both formats. |
| I-18 | No replacement objects, filters, alternate stores, lazy fetch, hooks or shell interpolation change inspected bytes | Current: hardened GitProcess builder and alternates policy. Required: route the batch reader through the same hardening and rerun existing replacement/promisor/alternate-store fixtures. |

## Snapshot reads, transport, lifecycle and performance

| ID | Invariant / condition | Current evidence and required proof |
| --- | --- | --- |
| I-19 | Inspect returns internally consistent knowledge/source metadata and hashes from one retained reading snapshot | Current: profiled inspection's three addressed hashes matched git show bytes. Required: preserve unknown-ID whole-request failure, external-source membership, missing ranges/hash:null, source deletion, mixed knowledge/source IDs, final newline/CRLF and whole-file identity precedence. It must not substitute author-basis bytes for captured current bytes. |
| I-20 | Inspect does not verify an author's unrelated full source tree | Required: count/identify verification roots by OID; legitimate strong pin of the newly captured reading snapshot must not conceal traversal of an author tree. Cost may scale with captured files; no unsupported constant-time claim. |
| I-21 | Batch framing remains synchronized for arbitrary object bytes and short reads | Required: multiple binary objects with LF/NUL, exact header OID/type/size/body/trailer validation, header overflow, checked size parsing, EOF at every boundary and nonzero exit. Git2.29 protocol only; no newer batch-command dependency. |
| I-22 | Batch transport cannot deadlock or leak an owned child on ordinary failures | Required: stderr pressure, broken pipe, malformed first response, early drop and child failure under bounded fixture supervisors. Drain stderr concurrently with bounded retained diagnostics; close/terminate/wait on normal teardown. Preserve private safe causes without raw knowledge/provider/process payloads. |
| I-23 | SIGINT preserves one cancellation response and terminates the real owned batch session | Current: signal listener exits the process and skips Drop. Required: real Git child exits when OS closes pipes; ordinary cleanup tests must not be mistaken for SIGINT coverage. No impossible parent-reaping promise after process exit; detached embedding drains are independent. |
| I-24 | Streaming avoids retaining all closure contents | Required: stream blobs through a bounded buffer, retain current tree plus OID bookkeeping only, reject impossible protocol sizes safely. Memory work scales with metadata/current object rather than total blob bytes. No persistent certificate cache or process pool. |
| I-25 | Command/schema/provider behavior outside the explicit audit boundary remains compatible | Required: unchanged JSON request/response shapes and query-config consistency/fallback behavior; status/config/help gain no audit. Preserve optional provider latency as a separate measurement, not Git or database query time. |
| I-26 | Performance claims are supported by reproducible measurements and deterministic work-count tests | Current: raw samples and counts in the baseline report. Required: fixed owned corpus, matched symbolized optimized builds, three warmed runs per operation, medians/ranges, before/after counts, separate full-check/reindex timings and provider work. Wall-clock targets are not ordinary test thresholds. |
| I-27 | Historical evidence and user workspace remain intact | Current: only design/plan/report files changed. Required: isolated implementation worktree, existing Engram store, no recreation of removed main knowledge files, no overwritten evidence or rewritten historical success claims. Update contracts/help/skill/decisions to describe the new detection timing. |

## Audit findings resolved in the documents

1. **Strong value type misuse:** lightweight retention returns unit, not RetainedTree; the latter keeps its complete-closure meaning.
2. **Post-publication verification:** full check uses verified incremental refresh, not ordinary refresh followed by auditing a potentially different revision.
3. **Gap before the diagnostic read:** add audited-revision equality guarding full check. The current read view is consistent internally but alone does not prove it was the revision just audited.
4. **Old roots and invalid Markdown:** derive the prospective valid root set, keeping last-valid invalid projections without retaining superseded/deleted valid roots in the audit set.
5. **Cancellation destructors:** process::exit bypasses Drop; prove real Git EOF shutdown rather than promise destructor-based reaping after exit.
6. **Hash replacement:** plain SHA-1 is not an approved substitute for collision-aware Git hashing; dependency feasibility is a mandatory gate.

## Completion rule

Each required item needs an actual regression test/result or an explicit platform/scope limit in the final implementation report. New tests must be mutation-checked. Run make check and make verify on the changed product tree, plus the host native runner if dependency/build inputs change. Until implementation, this matrix is a reviewed requirement set, not a claim that all conditions have passed.

## Current-tree baseline receipt

The following commands were run after the matrix audit, before any product implementation:

```text
cargo test --locked --offline --lib service::check
  7 passed, 0 failed
cargo test --locked --offline --test search_consistency --test knowledge_inspection --test source_inspection --test git_retention --test git_corruption --test cli_cancellation
  search consistency 7, knowledge inspection 6, source inspection 7,
  Git retention 11, Git corruption 3, CLI cancellation 4:
  38 passed, 0 failed
```

Total: 45 existing tests passed. This verifies the unchanged baseline's pinned-view, source-reading, retention, corruption and cancellation behavior. It does not exercise lightweight retain, verified refresh, batched transport or the new audited-revision guard, which are not implemented. The existing basis-preflight tests were also run successfully earlier in this session, as recorded in the baseline report; they were not counted again here. All four current document files passed local-link existence and whitespace checks (19 local links). No unchanged product-wide/native/Linux suite is implied.

## Implementation review corrections

Independent review found that old hash-object calls also validated tree structure. The final strong path keeps those exact Git checks through one additional batched tree-validator process over private transient metadata files. Noncanonical-mode decoder normalization is not a relaxation of strong-pin acceptance. Nested malformed-tree regression was observed RED then GREEN. I-22 now includes early BrokenPipe and external process-group watchdogs; disabling stderr draining caused a bounded test failure. Mutations removing structural validation, existing-ref rechecks, process-count work, spool permissions/cleanup/quoting also failed their expected regressions. Final gate results are recorded in the execution report.

## Execution evidence against all 27 invariants

The final `make verify` passed 926 root tests in debug and 926 in release, with no failures. The following maps each invariant to current implementation/review or executed tests; it is not a claim of exhaustive state-space or power-loss proof. Host/native receipt and platform limits are recorded in [report](report.md).

| ID | Current evidence |
| --- | --- |
| I-01 | `read_verification::all_commands_and_freshness_preserve_canonical_bytes`, document_refresh/search_consistency/cli_traversal and inspection suites |
| I-02 | E2E argv counts for unchanged reads, new bases and Markdown edits; unrelated damaged descendants remain unread |
| I-03 | CheckService whole/scoped tests, nonempty selection naming all files, CLI decoding and E2E whole/scoped checks |
| I-04 | `verified_unchanged_refresh_audits_without_sql_write`; separate reindex process/publication tests |
| I-05 | Boundary event ordering and `verified_refresh_failure_preserves_previous_publication`; real E2E edited knowledge retains old revision after audit failure; audit mutation caught |
| I-06 | Retain-before-BEGIN boundary tests, unchanged retention test, new-basis E2E and missing-root/provenance tests |
| I-07 | Invalid last-valid owner plus valid replacement/deletion E2E; prospective-root mutation caught |
| I-08 | Newer-revision rejection test; original-pinned-view and publication-after-pin tests; revision mutation caught |
| I-09 | Shared stat/hash recheck pipeline reviewed; process schedules for refresh/reindex, post-preflight replacement rollback and commit cancellation passed |
| I-10 | Provenance/search_consistency/source_inspection tests preserve authored basis/time while source-only edits change observations |
| I-11 | retain returns unit; root-type/missing-root tests and incomplete-child lightweight retention test |
| I-12 | Repeated/reopened pin corruption and missing/swapped object tests in both formats; strong closure mutation caught |
| I-13 | Separate unrelated-descendant and addressed-object corruption E2E; root corruption suites |
| I-14 | Light GC survival plus symbolic/conflicting ref tests; shared exact-ref/CAS implementation and concurrent pin tests |
| I-15 | Root/author/requested/missing-child classifications in provenance, git_snapshots, retention and corruption suites |
| I-16 | Git-oracle SHA-1/SHA-256 and binary fixtures; wrong-OID tests; sha1-checked compile/empty-object/SHAttered probe; hash mutation caught |
| I-17 | Decoder versus ls-tree oracle in both formats; strong nested malformed-tree rejection versus Git hash-object; Git keeps ordering/duplicate/path/mode/alias policy; structural-check mutation caught |
| I-18 | Existing replacement/alternate/promisor/environment tests; both batch processes reuse hardened GitProcess |
| I-19 | Knowledge/source inspection suites plus real snapshot/hash E2E; exact current bytes and source membership behavior preserved |
| I-20 | Inspect succeeds with damaged unrelated author descendant; only its new captured tree has a batch pin in argv count |
| I-21 | Binary multi-response framing; malformed/oversized/missing/wrong-ID/type/size/EOF/trailer tests under watchdog; OID/trailer mutants caught |
| I-22 | Early BrokenPipe, failed exit/private cause, Drop/reaping, stderr-pressure tests; external process-group deadlines and removed-drainer mutant; private spool cleanup tests |
| I-23 | `cancellation_closes_batch_pipes_and_real_git_exits` plus all foreground cancellation and once-response tests |
| I-24 | 64-KiB blob buffer and tree-only metadata spool reviewed; no blob body accumulation or persistent verification cache; private 0700/0600 spool and cleanup verified |
| I-25 | All eight public commands plus internal embedding drain, config/provider/fallback/help/version suites passed; JSON shapes unchanged |
| I-26 | Fixed corpus/cache/build inputs, three warmed before/after runs, separate argv counts, symbolized Rust samples, constant-per-closure process-count test; host-load/sampler limits disclosed |
| I-27 | Isolated branch; main status unchanged; no recreated main knowledge fixture; historical raw evidence copied byte-for-byte, not overwritten; contract/help/skill/decision updates and local links checked |
