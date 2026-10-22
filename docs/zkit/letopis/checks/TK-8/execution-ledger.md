# SDD ledger — plan: docs/superpowers/plans/2026-10-22-tk-8-git-snapshots.md

User chose Subagent-driven with “Subagent”. Implementation authorized on existing TK-8 worktree only. No merge/push/issue closure.
Branch base: 2a7e8db0758e3410b5ec4327848b7c06185907e4.
Task 1 BASE before dispatch: 2a29ab16603569bb95e0e5d63ce4942180050686.

## Preflight

| Tasks / interface | Produces / consumes | Finding |
|---|---|---|
| Task 1 self-consistency | A process/version; B raw object creation/read; C candidate closure/CAS; V cross-platform evidence | Files and exact public/private signatures agree; fixture-state check precedes deliberate live mutation; interim stubs allowed only in RED and absent at completion. |
| Task 1 internal A -> B -> C | GitProcess/root/object format -> ObjectDb -> Retainer, lib.rs factory/port used by integration tests | Same RepoPath/TreeOid and error/accessor contracts; references consume verified raw objects and exact format, no cross-task files/interfaces because plan has one task. |
| Task 1 vs global constraints/spec | Git argv/stdin and service-owned port -> concrete private adapter/root assembly | No shell/runtime/index/HEAD/config changes; per-command env/hook policy and pin namespace align. Physical durability and actual Git2.29 execution remain bounded. |
| Task 1 verification vs SDD gates | Implementer verification/evidence -> task review and final whole-branch review | Final acceptance/status and independent review records are controller finalization after gates; implementer must not claim review PASS beforehand. The plan's single whole-branch review coexists with SDD task-scoped spec/quality review. |

## Tasks

Task 1: pending — implementer dispatch next; A/B/C TDD and V platform/evidence; review gates outstanding.

Task 1: running — implementer /root/implement_tk8, gpt-6.1-sol/high, isolated context; owns planned implementation/tests/evidence. Engram revision48 records selected execution method.

Task 1: cycle A implementer reports GREEN — 6 private tests on host Git2.50.1;4 behavioral RED failures retained. Root/version/object-format/process IO/reaping and environment/write policy covered. CycleB running; task/final reviews pending.

Task 1: cycle B implementer reports GREEN — full root41 tests (existing30/private6/public5); public scenarios exercise both object formats. Exactbytes/literalpaths/prewritevalidation/replace/type/missing/corrupt covered. Explicit pin stub remains pending cycleC; no completion claim.

Task 1: cycle C implementer reports initial GREEN —6 retention tests, both object formats, real unretained-GC negative control, retained/external nested closure survives GC+pack/reopen, missing/corrupt child/ref-lock failure/concurrent pin/no-fetch trap. Full host/Linux checks and self-review running; no task completion yet.

Task 1: implementer self-review found/fixed two reproduced retention defects with RED/GREEN: dangling symbolic ref hidden by for-each-ref; OID-only visited tracking hid conflicting child types. Explicit symbolic-ref probe and type consistency added; gitlink no-dereference and owned-child cleanup tests. Host final verification/Docker build running; actual totals pending logs.

Task 1: implementation report DONE — commits9d9413b/2df19b1, finalHEAD2df19b12e0aeb20ba3ccab030326678ad61adee6; host+Linux49PASS/fmt/Clippy, hostile-routing13PASS. Controller verified source41/evidence30 SHA256 all match. Task review /root/review_tk8_task (gpt-6.1-sol/high) dispatched with full BASE..HEAD package; gates outstanding. ActualGit2.29/powerloss/x86_64 explicitly unverified.

Task 1: review failed —1Important, root trailing-newline trim can route writes into sibling repo, independently reproduced. NoCritical/Minor. Fix round1/5 begins at2df19b12e0aeb20ba3ccab030326678ad61adee6; resume original implementer with regression and refreshed verification evidence. Unverifiable actualGit2.29/powerloss/x86_64 are explicit scope limits confirmed in spec/report, not gaps in claimed executed platforms.

Task 1: fix round 1/5 (1 addressed,0open — trailing-newline repository routing; commits2df19b1..a6af7d7). Scoped re-review clean, controller verified41source/14new-evidence hashes.
Task 1: complete (commits2a29ab1..a6af7d7, review clean). All implemented/tested; final whole-branch gate and acceptance bookkeeping remain.
Deferred minors: none. Parked findings: none. Rulings: none.

Final review: 1Important — alternates/donor-GC breaks successful retention, both formats reproduced; noCritical/Minor. One final fix wave starts at a6af7d7b6536a65d85f11f6f02807a906b0e640a.
Ruling: Reject any existing Git objects/info/alternates configuration path, including empty/inactive files, at open and public operations rather than adding object materialization — ordinary service refs cannot protect donor GC and hash-object -w deduplicates against alternates; conservative presence refusal avoids a new parser and donor-path disclosure — cost if wrong: shared/reference clones need a self-contained clone or later support, and even inactive alternates configuration must be removed before use.
Ruling: Keep non-UTF8 checkout roots unsupported with a safe explicit discovery error and document the boundary — current root discovery already fails safely and byte-path support is not a specified TK8 requirement — cost if wrong: users with such Unix directory names must rename the checkout or await separate byte-path support.
Final-review declined list reconciled: all other excluded behaviors match explicit deferred/bounded spec requirements, not hidden omissions; see final-review.md. No additional PASS claims.

Final fix wave: commits92cf8bd/b344c08; scoped re-review confirms1addressed/0open, no newCritical/Important/Minor. Final review gate complete.
Controller finishing verification: cargo +1.95.0 test --locked --offline on unchanged final source:53PASS/0failed (fresh full run). Source41/amendeddocs2/priorreceipts2/historicalevidence44/newevidence19 hashes match before final administrative docs.

Final administrative acceptance: taskcard done/4criteria,plan16steps complete,EP1/roadmap/spec updated; schedule check PASS45tasks/333h/34weeks/0errors;45cards/173canonicalarchitecturelinks valid. Initial inline-checkbox-example counting check corrected to count only actual step lines; no product failure. Final source/tests/config unchanged since92cf8bd. Full task/review reports durably archived; final bookkeeping commit pending.
