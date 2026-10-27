# TK-29 — G-2 evidence matrix

Status: **G-2 not passed.** Functional implementation is delivered and the
maintenance workflow, cancellation protocol and schema contract are exercised on
the final tree. This matrix is not an unconditional continue: G-1/TK-17 remain
deferred (D-103), C-10 x86_64 remains the roadmap's TK-43 exception, and several
requirements have only historical evidence at an earlier revision. The task card
keeps the G-2 box unchecked and records the exact blocker.

## Evidence keys

| Key | Evidence |
| --- | --- |
| WF | `tests/maintenance_workflow.rs` at the final tree, SHA-256 `01726c747e30019c72029e54ccbd871f646f05f09195385bc94dfefe8191c8cc`; executed by both final runners and the focused run. |
| FOCUS | `.artifacts/tk29/focused-final-20261028T022000/maintenance.log`, SHA-256 `5a763be7c2907a0f37bcb4aee03e3a3bc5c886596ca6c8560302a12602354b50`; 1 passed, 18 raw `WORKFLOW_ENVELOPE` lines. |
| LFOCUS | `.artifacts/tk29/linux-focused-20261028T021000/maintenance.log`, SHA-256 `fcbae5af19693cc54299477eaebaf9318c8588499f43fb1cf7c7510fda4f23b0`; focused Linux container run, 1 passed. |
| SCHEMA | `.artifacts/tk29/focused-final-20261028T022000/schema-validation.log`; `tests/cli/run.py::validate_response` with `Draft202012Validator` and `X-1-cli.yaml`, 18 envelopes accepted (4 check, 4 config, 1 inspect, 1 reindex, 3 search, 4 status, 1 traverse). |
| HOST | `.artifacts/tk29/host-20261028-final-bb7847f/rust-check.log`, SHA-256 `3318234f8aa4ef1322e7b05f221fe80bfb6de79bb6e3d668d4e30df1f932a7ce`; macOS arm64 `make verify` PASS on final tree `bb7847f`, 844 passed / 0 failed / 10 ignored in each of dev and release profiles. |
| LINUX | `.artifacts/tk29/linux-20261028-final-bb7847f/container/rust-check.log`, SHA-256 `14d658d72c32fe8620eda3b05cddec75b0f20607c4f7d5856880ded3c0a1f982`; Linux aarch64 glibc 2.36 `make verify` PASS on final tree `bb7847f`, 844 / 0 / 10 in each profile. Two earlier full Linux runs failed an unrelated pre-existing `cli_runtime` BrokenPipe race (see report limitations), then fresh focused and full runs passed. |
| TK-XX | Prior report at `checks/TK-XX/report.md`; the linked commit is the last commit touching that report directory, not a claim that the report's captured product revision equals that commit. |

Classification meanings: **Current** = executed at the final tree in this task's
focused run or host/Linux runners. **Historical** = last accepted report at an
earlier revision; tests were re-executed by the final root suite but no new
independent receipt was captured. **Unverified** = no evidence for the stated
scope.

## Functional requirements

| ID | Requirement (short) | Evidence and revision | Coverage | Result | Open limitation |
| --- | --- | --- | --- | --- | --- |
| FR-1 | Shared knowledge is found across sessions | WF: two authored knowledge elements are found by separate CLI processes; [TK-14](../TK-14/report.md) @ `c8ede9e`; [TK-24](../TK-24/report.md) @ `d6657e8` | Current functional | PASS functional | Cross-session agent value, pilot benefit and G-1 remain unmeasured (TK-17/TK-32). |
| FR-2 | Agent saves knowledge by editing or command | WF: the test agent writes Markdown and the next process reads it; TK-14 | Current functional | PASS functional | Same pilot gap as FR-1; no autonomous-agent pilot. |
| FR-3 | Marked Markdown is canonical; letopis never edits corpus/source | WF asserts authored Markdown, source, Git index and HEAD are byte-identical after config/check/search/traverse/inspect/reindex/cancel; TK-24 | Current | PASS | Only exercised commands and files; no filesystem-wide invariant proof. |
| FR-4 | Path/anchor IDs, editable content, explicit directed links | WF uses `knowledge/decisions.md#cache`, `depends_on` and `external`; [TK-18](../TK-18/report.md) @ `761d3b9`; TK-24 | Current partial | PASS partial | Rename and ID-stability scenarios are historical only. |
| FR-5 | Includes meta-vertices; cycles forbidden | TK-18; [TK-23](../TK-23/report.md) @ `026254a`; [TK-19](../TK-19/report.md) @ `15ccc70` | Historical | PASS at captured revision | Not rerun at the final revision; no current cycle scenario. |
| FR-6 | Check/index validation; one invalid file does not block others | TK-18; TK-19; [TK-26](../TK-26/report.md) @ `8bc4fca`; [TK-28](../TK-28-status-acceptance/report.md) @ `5a420e2` | Historical | PASS at captured revision | The final root suite re-executes old tests, but no fresh invalid-file CLI receipt was captured in TK-29. |
| FR-11 | Delete/rename with incoming links is detected | TK-19; TK-28 | Historical | PASS at captured revision | Not rerun at the final revision. |
| FR-12 | Check whole corpus or paths, no file changes | WF whole-corpus `check` returns `errors:[]` and files stay unchanged; TK-19 | Current partial | PASS | Path selector and invalid corpus are historical only. |
| FR-13 | Search sees valid changes made before it starts | WF `check` then `search` see the same publication; [TK-25](../TK-25/report.md) @ `e8eb1a2`; TK-28 | Current partial | PASS partial | WF does not edit Markdown between publication and search; historical tests cover refresh. |
| FR-17 | Sources whole/range are addresses and graph nodes | WF inspect/traverse `src/cache.rs#L2-L4` with path/lines/hash; metadata-only envelopes; [TK-20](../TK-20/report.md) @ `ed3e924` | Current partial | PASS | Removed-final-reference behaviour is historical. |
| FR-18 | `source_snapshot` is required and must be an existing tree | [TK-20](../TK-20/report.md); [TK-21](../TK-21/report.md) @ `46456a0` | Historical | PASS at captured revision | WF uses a valid basis; no current missing/invalid-basis receipt. |
| FR-19 | Editing keeps the basis; explicit new basis detects changed bytes | TK-20; TK-21 | Historical | PASS at captured revision | Not rerun at the final revision. |
| FR-20 | Changed/missing provenance marks recheck | TK-20; TK-21; [TK-22](../TK-22/report.md) @ `3b27c95` | Historical | PASS at captured revision | TK-39 corruption/Git drill is pending. |
| FR-21 | Search returns links, revision and flags, never content | WF search result has IDs/path/lines/revision/flags in metadata-only envelopes; [TK-12](../TK-12/report.md) @ `8db4814`; TK-22 | Current | PASS | Only empty flags were exercised currently. |
| FR-22 | Hybrid search with FTS fallback and vector-lag safety | WF valid search; search remains available without valid config; `tests/cancellation_concurrency.rs` late-text test in HOST/LINUX; TK-22; [TK-27](../TK-27/report.md) @ `f007c88` | Current partial | PASS | TK-37 queue drill and real model quality are outside this task. |
| FR-23 | Search filters by recheck/source/revision | TK-22 | Historical | PASS at captured revision | Not rerun at the final revision. |
| FR-24 | Revision filter compares addressed bytes | TK-21; TK-22 | Historical | PASS at captured revision | Not rerun at the final revision. |
| FR-25 | Batch mixed inspect returns one snapshot; unknown ID rejects all | WF mixed inspect returns one snapshot for knowledge and source; [TK-24](../TK-24/report.md) | Current partial | PASS | Unknown-ID all-or-nothing is historical only. |
| FR-26 | Traverse modes, directions, types, depth and limits | WF `both`/`outgoing`/depth 1 with `depends_on` and `external`; TK-23 | Current partial | PASS | Full mode/direction/limit matrix is historical. |
| FR-27 | Full reindex; missing initial index is handled automatically | WF empty initial cache/index `reindex` returns revision 1; TK-25 | Current | PASS | Deleted-cache recovery and vector reuse are historical. |
| FR-28 | Changed files are detected and republished incrementally | TK-25; TK-39 (pending) | Historical | PASS at captured revision | No current edit/detect receipt; TK-39 drill pending. |
| FR-29 | Vectors match search text; late old result cannot represent new text | `tests/cancellation_concurrency.rs` in HOST/LINUX; TK-27 | Current partial | PASS | No dedicated TK-29 report for the concurrency test; TK-37 queue drill pending. |
| FR-30 | Retries are bounded, durable and reset by config/vector deletion | [TK-3](../TK-3/report.md) @ `c8f414b`; TK-27 | Historical | PASS at captured revision | TK-41 provider drill is pending; no current retry receipt. |
| FR-31 | Search/reindex can be cancelled keeping the last good state | WF cancelled `check` + retry; `tests/cli_cancellation.rs` (4 tests), `tests/cancellation_concurrency.rs` (5), private storage commit tests in HOST/LINUX; FOCUS/LFOCUS | Current | PASS on macOS arm64 and Linux aarch64 | x86_64/TK-43 and physical power loss/TK-38 are unverified. A cancelled response promises caller termination, not rollback; a commit that completed before process death survives. |
| FR-32 | Status shows revision, backlog, failures, locks and problems | WF status before/after reindex and with invalid/restored config; TK-28 | Current partial | PASS | Held-lock PID display is historical; WF observed no held lock. |
| FR-33 | Invalid/dangling problems persist while they exist | TK-28; TK-26 | Historical | PASS at captured revision | TK-38 power-loss drill is pending; not rerun fresh. |
| FR-34 | Content and prior basis are read from retained Git snapshots | WF `git show <inspect-snapshot>:<knowledge>` and `:<source>`, plus the declared `source_snapshot`; TK-24; TK-25 | Current partial | PASS | TK-40 restore and long-history cases are pending. |
| FR-35 | `created_at` survives edits and renames | [TK-8](../TK-8/report.md) @ `aa5367d`; TK-21; TK-24 | Historical | PASS at captured revision | Rename preservation is not rerun; TK-40 is pending. |
| FR-36 | A coding-agent skill is shipped | WF executes the documented config/status/reindex/read/cancel workflow; TK-14; TK-24 | Current partial | PASS functional | TK-17/TK-30/TK-32 pilot and TK-44 delivery are pending. |

## Constraints

| ID | Constraint (short) | Evidence and revision | Coverage | Result | Open limitation |
| --- | --- | --- | --- | --- | --- |
| C-1 | CLI only, one JSON, exit 0/1, unknown fields rejected | WF 18 envelopes validated by X-1 tooling; [TK-11](../TK-11/report.md) @ `65c2602`; TK-24 | Current | PASS | Unknown-field rejection is covered by historical tests and the final root suite, not a new envelope in WF. |
| C-2 | One project, machine and Git worktree | WF uses one disposable repository; TK-11 | Current partial | PASS | Multi-worktree/ACL behaviour is an explicit non-goal; no new negative test. |
| C-3 | Canonical marked Markdown under Git; DB copies no document text | [TK-9](../TK-9/report.md) @ `360bbf7`; TK-25; TK-36 (pending); TK-43 (pending) | Historical | PASS at captured revision | TK-36 disk-growth and TK-43 delivery receipts are missing. |
| C-4 | Rust, rusqlite, SQLite cache, separate vectors, FTS5, vec1 | [TK-1](../TK-1/report.md) @ `ea86ead`; TK-9; HOST/LINUX `native.log` | Current | PASS on arm64 | x86_64 and reproducible delivery remain TK-43. |
| C-5 | SQLite 3.51.3+, WAL, `synchronous=FULL`, fullfsync, foreign keys | TK-1; TK-9; HOST/LINUX `native.log` | Current | PASS on arm64 | x86_64 remains TK-43. |
| C-6 | YAGNI/KISS: no speculative mechanisms | This report's architecture review plus prior task reviews | Current review | PASS review | Not a runtime test; Tasks 1–4 product code were not independently re-reviewed by this task. |
| C-7 | DDD/hexagonal boundaries; no permanent worker; per-call process, flock, detached drain | WF runs separate CLI processes; TK-11; TK-43 pending | Current partial | PASS | Final review is scoped to Task 5 test/docs; product structure evidence is historical. |
| C-8 | Git stays external; changes are ordinary file edits | WF operates on a real Git repository and reads via Git; [TK-8](../TK-8/report.md); TK-25 | Current partial | PASS | TK-39 Git-change drill is pending. |
| C-9 | Cache is recreatable; vectors separate; config is a file; no corpus writes | WF reindex from empty initial cache/index, config edits by the test agent, authored files unchanged; TK-9; TK-25; TK-27; TK-28 | Current partial | PASS | Cache deletion/recovery and TK-38/TK-40 drills are pending. |
| C-10 | macOS/Linux glibc, arm64 and x86_64 | HOST macOS 26.6.2 arm64; LINUX Linux aarch64 glibc 2.36; TK-43 exception | Current partial | PASS on arm64 | **x86_64 not run and remains the roadmap's TK-43 exception.** Windows/NFS unsupported. |

## Gate decision

- **G-1 / TK-17:** no pass. TK-17 has no report; the user deferred minimum-hardware
  and pilot measurements (D-103). This is a prerequisite visible in the TK-29 card,
  not waived by the functional delivery.
- **C-10:** x86_64 is explicitly not assessed; TK-43 owns delivery-platform proof.
- **Historical rows:** FR-5, FR-6, FR-11, FR-18, FR-19, FR-20, FR-23, FR-24, FR-28,
  FR-30, FR-33, FR-35, C-3 and parts of C-7/C-8/C-9 have no new independent receipt
  at the final revision. The root suite re-executed their tests, but that does not
  replace the captured reports.
- **Linux repeatability:** the final Linux aarch64 runner passed, but two earlier
  full runs failed the pre-existing `cli_runtime` BrokenPipe race. No stable
  repeatability claim is made.
- **Decision:** G-2 is **not passed** and the continue to TK-30 is not granted by
  this report. Functional implementation delivered: yes. Formal gate status:
  pending the missing prerequisites and evidence above.
