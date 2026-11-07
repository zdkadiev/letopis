# Issue 65 follow-up execution

Spec: [current-revision design](../specs/2026-11-05-issue-65-refresh-design.md).

1. Inspect current code and old evidence; reproduce runner compatibility gaps.
2. Add failing regression tests, update full-index scenario/writer/oracle,
   mutation-check and run affected suites.
3. Pin the unchanged release; run bounded model-free 1k/10k/100k surveys.
4. Profile expensive operations, using isolated temporary instrumentation if
   native attachment is denied; record overhead and causal controls.
5. Implement only a confirmed focused optimization, if safely justified; prove
   behavioral correctness and compare ordinary pinned builds.
6. Run required verification, review the complete diff independently, and save
   a fresh report/evidence index with exact commands and limitations.

## Progress and rulings

- Original tasks 1–6 are complete in the historical 2026-10-30 plan; do not redo
  their implementation or attribute their measurements to current code.
- Current baseline is clean product commit `52ed491`; initial 60-node smoke
  independently reproduced the obsolete-command failure.
- Scenario, publication oracle and writer regressions each failed before fixes.
- Native profiler probe: samply `Unknown(1100)`; `/usr/bin/sample` unable to
  examine owned PID. Preserve receipts and use isolated timing instrumentation.
- Baseline Python performance suite: 81 tests, 4 permission errors (loopback bind
  and ps), 8 explicit integration skips. These are environment limits, not passes.
- Tasks 1–5: complete. Updated the runner with three regression tests and three
  rejected hand mutants. New ordinary surveys cover all nine scenarios at 1k and
  10k; 100k first publication is censored at 120 seconds, so initialized scenarios
  remain explicitly unassessed.
- Isolated Rust timing dumps identified Git object staging. Five focused Rust
  tests and four rejected hand mutants protect the loose-object reuse change.
  New/packed objects preserve staged publication; live source captures are not
  cached. Matched inspect medians fall 78.04% at 1k and 79.84% at 10k (n=10 each).
- Independent code review found and closed one safe-diagnostic issue through
  RED→GREEN. Scoped review approved the sync-helper extraction. Independent data
  review verified all comparison numbers and 106 product input hashes; corrected
  percentage wording, inspection-vs-search oracle scope and evidence links.
- Ruling: mutation checks in the temporary clone shared Cargo's target directory,
  leaving a deliberately mutated debug test executable. The first final suite
  exposed it. A focused replay reproduced the stale executable; touching the
  restored source forced recompilation and the same test passed. Preserve those
  receipts separately; future mutation work must use its own Cargo target.
- Actual source bytes match the measured optimized release. Changes are left in
  the requested checkout because its Git metadata is read-only. The measurement
  commit exists only in the ignored isolated clone; no push/merge/issue publication
  or closure was performed.
- Task 6: fresh release Git tests (52), strict Rustdoc, no-helper integrations (4),
  generator tests (25) and Python performance suite (72 pass / 4 permission errors /
  8 integration skips) are recorded. Final root verification after recompilation
  and its platform limits are in the [report](../../zkit/letopis/checks/issue-65/current/report.md)
  and its verification/evidence receipts.

## 2026-11-06 extension: 100k and real Ollama

The user requested another 100k attempt and explicitly authorized local Ollama
embeddings. The 600-second initial-publication window completed in 162.648 s,
with independently verified 100k knowledge nodes, 100k sources and 500k edges.
The general copy-per-scenario survey was interrupted after successful initial,
search and first-inspect records; all owned index locks were observed released.
A direct continuation used the verified already captured fixture: three valid
samples for each read command and one valid full index (207.402 s).

The locally installed embeddinggemma model was selected. Direct loopback access
returned PermissionError/errno 1. A separate 60-node real-provider probe exhausted
60 embedding attempts and produced zero vectors; its successful FTS query and
drain exit are not hybrid/model performance evidence. No 100k failed drain was
launched against that blocked endpoint. User authorization is present; runtime
network permission is the blocker.

The [new report](../../zkit/letopis/checks/issue-65/2026-11-06-100k/report.md)
preserves the larger-budget results, small-sample limits and separate Ollama
failure receipts. Product source and dependencies are unchanged in this extension.

## 2026-11-06 extension: check/status and Samply

The user requested Samply specifically. Direct launch and owned-PID attach both
fail before target startup with Mach bootstrap error 1100. Running outside the
sandbox was automatically rejected by the session's `sandbox_approval: false`
policy. A prepared normal-Terminal launcher captures and validates both commands;
its successful native recording is still pending. No further product optimization
was implemented. Separate isolated Rust CPU measurements identify repeated
historical object decode and tree validation as the dominant work. The findings,
proposals, receipts and reproduction command are in the
[check/status report](../../zkit/letopis/checks/issue-65/check-status-profiling/report.md).

## 2026-11-06 native continuation ledger

- Native recording: successful check/status captures in
  `.artifacts/issue-65/samply-terminal-20261106T095911Z`; independently validated
  CLI outputs, graph counts, usable symbolicated samples and profile hashes.
- Decision: implement the smaller structural-validation memo after fresh content
  verification; defer historical payload reuse to preserve corruption observation.
  See the spec extension. All current user edits remain in the requested checkout;
  mutation runs use a separate copied source and Cargo target, never the root target.
- Steps: baseline verification; RED reuse regression; bounded reader metadata and
  integrity/failure/audit tests; mutation controls; required root verification;
  immutable release and alternating ordinary before/after 100k measurements;
  post-change Samply stacks; independent final review and new evidence report.
- Real Ollama 60-entry probe passed with 60 vectors / zero failures. A separate
  100k corpus copy is being drained with the pinned pre-memo release. Its resource
  scope excludes the Ollama daemon/model; shared-host activity is recorded.
- Memo implementation: first reuse test failed with 2 structural checks versus
  expected 1, then all 57 Git unit tests passed. All five hand mutants were
  rejected: disabled reuse, ignored content identity, cached invalid structure,
  exceeded metadata budget, and omitted full-audit structure validation. Restored
  source bytes match the pinned product and requested checkout.
- Measurement-only clone commit: `3d820de41ee12385bc734ac4ceeb56b78ba379f9`;
  immutable binary SHA-256 `9a3e2b70a907d135604ad04e6491e358d07575e987c1457d514a1820c2d2ba6a`.
  Main checkout remains uncommitted; all 106 inputs are captured separately.
- Independent final code/contract review: no blocking or minor findings. Full
  root verification is still running; no completion claim is implied by review.
- Samply 0.13.1 reinstall succeeded. New native check/status profiles validated
  after reinstall; repeated structural validation has no observed samples there.
  Profiled timings are not an ordinary before/after speedup comparison.
- Python suite: 84 run, 76 passed, 8 explicit integration skips in the clean pinned
  clone. A dirty-root attempt had two fixture-precondition errors, retained in
  its log; the runner intentionally refuses uncommitted product build pins.
- Final actual-tree `make verify V=1`: passed, including strict Rustdoc and both
  debug/release root suites. Each library profile reports 601 passed / 9 ignored;
  the complete log contains 2405 successful test executions and 22 ignored
  executions including repetitions/examples (not unique tests). The earlier
  baseline verify was interrupted during release testing after debug/Rustdoc;
  it is not labeled a completed `make verify`.
- Native recordings exist on both pinned releases. The original prepared
  launcher used an old default Python for its post-capture validator; both first
  profiles were independently validated with Python 3.12. The new launcher uses
  Python 3.12 explicitly and completes end to end with the reinstalled Samply.
- Real provider search is confirmed independently: zero FTS matches for a probe
  token, five distinct knowledge URIs returned, and native stacks show vector
  retrieval. Three partial-coverage calls and a separate Samply trace are retained.
  This does not imply 100k vector coverage or model relevance quality.
- Ordinary comparison completed: all 12 calls valid, three alternating pairs per
  command. `check` median 42.451972 → 32.037880 s (24.53% lower); `status`
  42.218257 → 31.566077 s (25.23% lower). Both sides shared the ongoing real
  Ollama drain; no compilation/tests overlapped this ordinary campaign.
- After mutations, the isolated test target was explicitly rebuilt from restored
  source and all 57 Git tests passed again; no mutated executable is retained as
  the candidate. Product release and root verification use separate targets.
- At 10:59:12 UTC, the original drain segment was deliberately censored at 51024
  vectors before its 3600-second timeout. The owned child was stopped, its
  committed attempt count observed, and terminated only at zero reservations / zero
  recorded errors. Read-only observations and SIGSTOP/SIGCONT/SIGKILL receipts are
  retained. No DB writes, attempt refunds or configuration changes were made by
  the controller. The first runner's nonzero/AssertionError is expected censoring,
  not successful completion or a provider failure.
- Same-config continuation started via `resume-ollama.py`, tool session 55004,
  with 7200-second budget and 51024 existing vectors / zero attempt rows. Do not
  change max_retries to resume: the complete configuration participates in identity
  and would reset existing vectors. Full coverage remains pending.
- 12:26:44 UTC: same-config continuation completed successfully with 100000 vector
  keys and zero attempt rows. Independent native vec1/SQLite coverage confirms
  100000 actual 768-dimensional vectors, zero missing/mismatched mappings, wrong
  widths, orphan hashes, uncovered knowledge, recorded errors or reservations.
  Three full-coverage mixed searches, model-enabled check/status and a genuine
  full-coverage Samply search profile are now running in the queued continuation.
- Full-coverage continuation completed all ordinary oracles and the genuine Samply
  search recording. Three mixed-channel searches: 111.853324, 111.937798 and
  111.006796 s (median 111.853324). Each includes the one independently known FTS
  result plus four valid nonlexical knowledge URIs. Enabled-model check/status:
  32.128565 / 33.395601 s, zero findings/backlog/errors/locks.
- Population took 8519.670992 s including a 5.810252-s restart gap; two active CLI
  intervals sum to 8513.860739 s. This is one controlled split population, not an
  uninterrupted latency sample or a production throughput verdict. No retry or
  config policy changed, no attempt was refunded, all 100000 native vectors map
  to current knowledge. Final source106 and corpus4334 hashes remain unchanged.
- Full search Samply: candidate eligibility 96.91%, Markdown parsing 53.64%,
  historical hashing 20.02%, current source hashing 8.40%, vector retrieval 1.90%
  observed CPU (inclusive scopes overlap). Follow-up drafts retain these costs;
  no additional search/drain optimization is shipped in this change.
- Independent full-data review confirmed final native vectors, hashes, coverage,
  timings, RSS, CLI oracles, full native stacks and wall-window counts. One report
  claim was narrowed: ordinary result membership proves vector contribution and
  retention of the expected lexical hit; execution of both retrieval paths is
  separately evidenced by the native mixed-query trace (FTS expression 23 stack
  observations, BM25 20, vector retrieval 2340). No runtime code changed.
- All implementation, measurement and required verification steps are complete.
  Current source matches the verified/pinned 106 inputs, corpus4334 inputs remain
  unchanged, historical evidence hashes are preserved. Root remains reviewable
  and uncommitted as requested; no integration/publication is performed.
