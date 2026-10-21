# TK-5 execution report

Scope: executable P-1 protocol/forms and supplied-record verification only. **No real pilot, coding agent, embedding/model request, utility/resource measurement or improvement claim.** Base `main@6fd2d188b22408211c3f6e388db5290665b98e21`; branch TK-5 and dedicated native worktree. Work is prepared on branch TK-5; merging, pushing and issue closure require a separate integration request.

## Deliverables and acceptance

| Requirement | Concrete artifact / observable result |
|---|---|
| Same agent, independent conditions, fresh sessions, empty memory | [Protocol](../../pilot/protocol.md) sections 1–4; study/session forms; validation rejects shared workspace/home identities, session reuse, prefill and code/memory discontinuity. Actual isolation needs operator preflight evidence. |
| Correctness and three criteria with full token accounting | Protocol section 5; `check.py` reports all outcomes, repeated investigations/revalidation/unknowns, agreement opportunities and all-model input/output usage. Cached/reasoning subsets are not double counted. Missing usage stays null; unsuccessful tasks cannot be dropped. |
| Task selection and early/final report forms | Prospective real Letopis chains, reuse_from and frozen rubric/prompt/check evidence. TK-17 shape 2×2×2; TK-30 shape 3×3×2. [Forms](../../pilot/templates/report.md) are deliberately unready until filled. |
| No leaks or optimistic reinterpretation | Operator sandbox/canary steps and independent HOME/state; checker only validates supplied metadata/files, not the actual sandbox. Demonstration/incomplete/correctness-not-preserved/descriptive statuses; no automatic winner/threshold/significance. |
| P-2.7 handoff | Raw operation/URI/bytes/timestamps/duration/group/error/evidence rows; unknown profile values retained. No resource measurements here. |

## Verification

`python3 -B tests/pilot_protocol/run.py --evidence NEW_DIRECTORY` executes all current unit/actual-CLI tests, the supplied early demonstration, a generated final 18-slot demonstration, five individually broken datasets and roadmap consistency. It hashes every current source/form/example and every receipt artifact, verifies immutable source inputs and all 60 unchanged TK-1…TK-4 source hashes against their committed PASS receipt. Those old Rust suites are not re-executed by TK-5.

- [macOS receipt](macos/manifest.json): 31 tests, five negative controls, both example shapes — PASS.
- [Linux receipt](linux/manifest.json): 31 tests, five negative controls, both example shapes — PASS. Both platforms have identical current input hashes. The final audit verified all 22 current source hashes, 78 raw artifacts per platform and 332 local Markdown links; roadmap consistency, three mechanics models/15 guard mutations and 17 resilience arithmetic checks pass. One independent [review](review.md) completed; its three findings are fixed with RED→GREEN regression evidence and fresh platform receipts.
- Negative controls: unreported request ID, reused session, prefilled memory, corrupted evidence and silently suppressed failed task. Each actual CLI run must exit 2 with the exact relevant error; no repair or unrelated-error acceptance.
- [Task 1 RED](tdd/task1-red.txt): missing validation module before implementation; initial [Task 1 GREEN](tdd/task1-green.txt), 10 tests; task completion reached 11 after the opaque-model boundary regression.
- [Task 2 RED](tdd/task2-red.txt): missing report/CLI module before implementation; combined GREEN, 25 tests at that point.
- Two additional boundary regressions: opaque auxiliary model names and malformed types; evidence/session symlink loops. Their RED→GREEN records are retained; suite before final review had 27 tests. Final review added four tests (31 total), reproduced hidden incomplete acceptance evidence and integer timing overflow, and hardened blank forms. [Review RED](tdd/review-fixes-red.txt) and [GREEN](tdd/review-fixes-green.txt) retain both sides of the fixes.

Example token totals (480 / 462 including 12 embedding tokens) are **authored fixture values**, not observed savings. Operator semantic annotation, factual task selection, model configuration, request capture completeness, real process sandboxing, future live exporters and all actual TK-17/TK-30…TK-33 runs remain unverified here. G-0 stays open until TK-6.

## Author choices

- Independent chains reset memory; within a chain code and memory follow each condition's own history. No across-chain learning conclusion.
- Failed/incomplete predecessor blocks dependent followups, retained explicitly as not_run; this conservative rule prevents an invalid starting state from becoming a cheap successful comparison.
- No agreement opportunities or unknown annotations/usage mean an incomplete comparison, not perfect scores or zero cost.
- Python 3.11+ stdlib provides a small offline checker; the existing Linux image is used for Python portability only.
- Preserve the shared project Engram outside all future pilot conditions, and preserve this task's worktree/evidence until integration.

No product architecture, public CLI schema, runtime/provider behavior or accepted decision was changed. The protocol formats are experimental observation forms. The checker has no privilege boundary against a hostile concurrent writer and cannot establish the truth of supplied logs or locators.

Pre-review receipts remain under `before-review/` and are historical, not current-source proof. Current manifests include all updated protocol/forms/tests. [Verification summary](verification.json) and [review dispositions](review.md) state the final evidence and every boundary left for later pilot tasks.
