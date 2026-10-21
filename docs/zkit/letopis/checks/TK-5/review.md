# TK-5 independent review and author disposition

One fresh whole-change review by `/root/review_tk5` (gpt-6-astra, high). Verdict: **With fixes**; no Critical. The reviewer independently ran the then-current 27 tests and verified both platform manifests (22 source files and 78 raw artifacts each), with identical input hashes. The reviewed package contained 189 new files, including evidence; the implementation was uncommitted at `main@6fd2d188b22408211c3f6e388db5290665b98e21`.

This document records the received findings and set-asides, then the author's dispositions. It is not a second review. The author fixed all three findings in one pass; [regression RED](tdd/review-fixes-red.txt), [GREEN](tdd/review-fixes-green.txt) and fresh macOS/Linux receipts establish 31 passing tests after fixes. Pre-fix receipts remain in [before-review](before-review/verification.json). No deferred Minor remains.

## Findings

| Received severity / finding | User-visible effect and disposition | Verification |
|---|---|---|
| Important: null acceptance evidence hidden by another false check | A task could be failed while data_complete=true despite an unassessed check. Fixed: report planned/assessed/not_run/unresolved check IDs separately; unassessed attempted checks force incomplete_data without erasing the failed outcome/correctness flag. | `test_failed_check_does_not_hide_other_unassessed_acceptance_evidence` failed before fix, passes now. Deliberately not_run checks remain distinct. |
| Important: large integer operation time causes OverflowError | `math.isfinite(10**400)` attempted float conversion, yielding CLI exit 1/traceback instead of its JSON contract. Fixed: integer finiteness needs no float conversion; only floats use isfinite. No arbitrary duration ceiling added. | `test_large_integer_profile_times_do_not_overflow_float_conversion` failed for both timing fields, passes now; infinity/NaN/bool/negative rejection remains covered. |
| Minor: forms retain plausible fake experiment metadata | **Author re-grade: Important.** Fake agent/model revisions, code/prompt hashes and pre-filled memory assertions could be accidentally retained in a real trial, invalidating reproducibility of the main deliverable. Fixed all experiment-specific form values to explicit fill markers/null assertions; executable demonstration stays separately labelled. This is a correctness risk in the operator form, not presentation polish. | `test_operator_forms_replace_experiment_specific_fixture_values_with_placeholders` failed before fix, passes now. |

## Every reviewer set-aside: author rulings

These are protocol choices/limits, not empirical validations. All are also recorded in the execution ledger.

1. **Actual agent/provider execution and utility:** remain TK-17/TK-30…TK-33; zero executed by this checker. Cost if wrong: real utility or exporter problems first emerge in those trials.
2. **Prospective, representative task selection:** the operator freezes selection/rationale before running; validation only checks supplied evidence. Cost: biased task selection can still yield an unrepresentative report.
3. **Actual isolation, global-memory absence and model identity:** preflight/canary/config evidence and operator review are required; metadata does not prove OS isolation. Cost: contamination or an incorrect model can invalidate the trial.
4. **Requests omitted from both supplied inventories:** cannot be discovered from absent evidence; require an independently captured complete source. Cost: hidden usage can undercount cost.
5. **Semantic annotations, correctness evidence and locator truth:** require rubric-based operator review, beyond checksums. Cost: valid hashes can accompany false or mistaken annotations.
6. **Several prompts inside one evidence file:** hash covers the whole file; locator identifies the intended prompt and must be reviewed. Cost: locator mistakes can make paired inputs differ.
7. **Unresolved research establishes a fact for later classification:** retained with an explicit definition: unresolved concerns the episode classification; fact discovery itself must be evidenced. If discovery is uncertain, do not assert that fact in the established inventory; preserve uncertainty in trace/report and finish annotation before comparison. Unresolved blocks complete comparison. Cost: operator misclassification remains possible; the checker does not infer factual truth.
8. **Stop after any unsuccessful immediate predecessor:** retained, even if reuse_from names a different earlier task, because code is sequential within each condition. Keep every blocked slot as not_run. Cost: conservative stopping reduces the usable sample.
9. **Chain resets, order and diverging code:** independent chains reset; each condition continues its own code; document order/carryover limits. Cost: no claim about across-chain learning or isolated causal memory effects.
10. **Unknown cached/reasoning subset with known totals:** retain complete normalized input/output totals and unknown breakdown. Cost: detailed cache/reasoning analysis can remain unavailable.
11. **Unknown workload fields do not block P-1 completeness:** expose profile completeness separately for P-2.7. Cost: resource analysis cannot use missing workload measurements as if known.
12. **No automatic per-task Markdown renderer:** manual report plus every JSON session row meets the current forms requirement. Cost: more operator work and transcription risk.
13. **Hostile concurrent evidence replacement/performance:** no security, capacity or performance claim; read-only confinement and hashing check supplied files. Cost: an adversarial writer can defeat a time-of-check assumption; no untrusted concurrent writer may be treated as safe by this tool.
14. **Pending task/roadmap/report status:** completion metadata is updated after the fix suite and evidence checks; TK-5 becomes done, G-0 remains open for TK-6. Cost: bookkeeping drift if future changes fail to update summaries.

## Outcome and remaining boundary

31/31 tests and five negative controls pass on macOS and Linux with identical current inputs. All 60 prior runtime source hashes match the committed TK-4 PASS receipt; Rust suites were not rerun. Review fixes have direct RED→GREEN evidence; there was no second reviewer. All synthetic values, including 480/462 tokens, are authored fixtures and cannot demonstrate measured savings. See [execution report](report.md), [protocol](../../pilot/protocol.md), and [verification](verification.json).
