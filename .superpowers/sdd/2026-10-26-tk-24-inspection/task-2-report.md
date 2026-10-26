# TK-24 Task 2 implementation report

**Status:** DONE_WITH_CONCERNS. Implementation acceptance is complete on the tested host checkpoint; final independent whole-branch review and any follow-up belong to the controller. The concerns are unrun Linux/real-pilot gates, not observed host failures.

**Base:** `0f8f92b205f6db48a188cc130c5a6e1190512e3d`; Task 1 approved implementation/fix: `bb6b39e9ef5a60d2f65e3da2ec16d97a2c742865`.

**Task 2 implementation commit:** `094c174accfe16d246645d780cd7717b3862dc9d` (skill, tests, CLI runner, workflow and README). The host runner captured its pre-commit dirty source at Task 1 HEAD and hashed all 205 inventoried inputs. That measured tree is byte-identical to the implementation commit except the README prose correction described below.

## Delivered behavior

- `tests/cli_reading.rs` invokes the production binary for mixed knowledge, local range, declared missing local source, opaque remote source, normalized duplicate and whole-batch `not_found` after the last source declaration or knowledge file is removed. Assertions require one JSON line, empty stderr, exact exit, metadata-only keys, independent SHA-256 literals and byte-for-byte Git reads from the returned retained tree.
- `tests/cli/run.py` now requires actual `inspect_mixed` and `inspect_removed` receipts, validates the selected X-1 InspectOutput success/error schema and checks the mixed source classifications. `tests/cli/test_runner.py` verified collector rejection when these required receipts are absent. The collector's old full-source-unavailable limit was replaced.
- `skills/letopis/SKILL.md` gives current request forms and Git reads, Markdown writing, a verified complete source tree through an isolated temporary index, source recheck, rename/repair and live-read limits. `tests/cli/README.md` separates this acceptance from historical TK-11/TK-14 receipts. `tests/cli/tk24_workflow.py` executes the no-provider agent procedure in a disposable Git repository.

## Evidence and checks

| Criterion | Evidence | Result |
| --- | --- | --- |
| Mixed CLI metadata and no partial result | `cargo test --locked --offline --test cli_reading binary_inspect_`, new binary tests | 4 passed, 0 failed on restored product source. New tests passed immediately against already implemented Task 1 behavior; no synthetic product RED claimed. |
| Test sensitivity | `/private/tmp/tk24-mixed-hash-mutation-compiled.log`; retained `.artifacts/tk24/workflow/compiled-mutation.log` | A compiling change from selected-byte SHA-256 to SHA-256 of `wrong` made the exact mixed-hash assertion fail. Original source restored; `git diff -- src` empty. A preceding `hash: None` candidate was rejected at compile time by unused-variable/import lints and is not counted as a behavioral control. |
| Collector RED/GREEN | Prepared Python 3.12.0/jsonschema 4.26.0 `-B -m unittest discover -s tests/cli -p test_runner.py` | Updated collector fixture first failed two tests against the old case map; after collector change, 20 tests passed. |
| Deterministic real CLI workflow | `.artifacts/tk24/workflow/workflow.json`, source `tests/cli/tk24_workflow.py` | PASS. Initial tree `431b5aa46a31547b154cc3bb690b91df55d5d862`; verified edited-source tree `49ff923ee20c7e6506a0aab0c73dc6ed75dcae76`; distinct reading snapshots recorded. Real Git index SHA-256 remained `2703f345923509a5102374ea9a783ae2367eb2596b4a29fd66dc19e80bbd1ac8`; HEAD remained `aa08c376c28586fc8db26edaa93bd25ea14e154b`. |
| Skill reference application | `skill-baseline-report.md`, `skill-after-evaluation.json`, `skill-after-response.md` | Baseline missed exact current request shapes and temporary-index recipe. One fresh after-update Luna trial used guide SHA-256 `18a0efb6721f93cdd08d58b2d4e893b16f79a61e4c2b5f6f91ba145eb8531a18` and supplied them. This is reference application, not an executed pilot or product test. |
| Final host runner | `.artifacts/tk24/host-20261026-task2/manifest.json`, `root-check.log`, `root-check.log.stderr`, `product-reading-responses.json` | PASS, exit 0. `make verify V=1` exit 0 with fmt, all-target check, Clippy `-D warnings`, strict Rustdoc, debug and release root tests. Debug root: 697 passed, 0 failed, 5 ignored; release root: 697 passed, 0 failed, 5 ignored. Each profile has 49 root executable invocations plus one Doc-tests result block. Eight nested child-helper blocks per profile each passed one selected test with 39 filtered; they are excluded from root totals. |

The host runner executed 42 recorded commands and captured 15 real `PRODUCT_READING` responses, seven product transport responses and seven thin-slice responses. It selected the actual InspectOutput success/error schema for mixed and removed cases; command-specific validation passed. The 205 input hashes are in `manifest.json`, whose SHA-256 is `229530e986928de9db2f7670129e7995c2b4ed96fc87f0a2509c2e4d6b04ea00`. The retained workflow receipt SHA-256 is `bac77951e101508e22f8eb4e0a0619f859a4b66364f3c0d5b628abac7870adf3`.

After the host run, only `tests/cli/README.md` changed to label old TK-11/TK-20 commands as historical. `.artifacts/tk24/host-20261026-task2/doc-only-hash-delta.json` records its tested hash `a06764c4bfc0b48a753d41a12efc3d5ddae30f38099ae485ccfb666fccc234f4`, final hash `062fd7db416faff29df4e3a4e251e3823cc7112807f679dddd00e11419bfab88`, and all other 204 runner inputs byte-identical. This is a documentation-only final delta; the tested manifest is not presented as a hash of the corrected README. Per AGENTS.md, affected links and instructions were checked instead of rerunning unchanged product suites.

The fixed workflow checked write → check → search → traverse → mixed inspect and `git show` reads; source edit → `source_changed` → reread → isolated temporary-index basis → check; rename → dangling-link diagnostic → repair → check. It performed no provider request or model evaluation. `workflow.json` retains literal argv, stdin, responses, exits, tree identities and real index/HEAD before/after hashes. The original disposable execution was `/private/tmp/tk24-workflow-1`; the local receipt copy is under `.artifacts/tk24/workflow/`.

## Architecture and limits

Task 2 changes no product `src/` file. Task 1's `InspectionService` remains in `src/service/inspection.rs` behind the application-owned `Inspector` from `src/service/mod.rs`; `src/cli/handlers.rs` delegates to that interface, `src/cli/output.rs` serializes the same metadata-only DTO and `src/lib.rs` retains concrete constructor wiring. No direct CLI-to-Git/storage call, new runtime, storage schema, or adapter layer was introduced. Domain remains I/O-free. This is a physical-file/import/visibility check in addition to fmt/Clippy, not a claim based only on lints.

The proof covers host execution and fixed bytes. Linux, native storage runner, historical proof harnesses, real provider/model quality, resource targets, handled cancellation, TK-17/G-1/G-2 and TK-25–29/TK-44 are not claimed by this task. Native storage runner was not triggered because no storage/native/build input changed. Final whole-branch review, Engram write and any GitHub action remain controller-owned.
