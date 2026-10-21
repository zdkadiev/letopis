# SDD ledger — plan: docs/superpowers/plans/2026-10-21-tk-7-markdown-domain.md
Execution: Native explicitly selected by user; spec and plan approved. Base b109a905868dba35ba00aa8d3552839fa78b7740.
Pre-flight: Task1 → Task2: validated IDs/values/edges match consumed signatures; no conflict.
Pre-flight: Task2 → Task3: full-file events retained; planned Vec<Element> to Vec<ParsedElement> migration explicit, no placeholder search fields.
Pre-flight: Tasks1/2 → Task3: Sha256 and typed metadata shared; no duplicate text/hash pipeline.
Task 1: in progress; BASE b109a905868dba35ba00aa8d3552839fa78b7740.
Task 1: RED observed: domain_values compiled; 0 passed/6 failed on pending constructors/hash; task1-red.log. Implemented validated values/edges after RED.
Task 1: complete (commits b109a90..0c32513, tests: cargo test --locked → test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
Task 1 verification detail: 6 integration tests PASS; task-done reports the final empty doctest target as 0 tests (not a test-count total).
Task 2: tests/golden fixtures written before parser behavior; BASE 0c32513.
Task 2: RED observed separately: 9 element tests and 4 link tests fail with parse stub returning no corpus; implementing scanner/metadata/link adapter.
Task 2: complete (commits 0c32513..524231f, tests: cargo test --locked → test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
Task 2 verification detail: 19 integration tests PASS plus fmt/clippy. Task3 tests/golden search values written first; planned output migration uses pending empty search values solely to obtain behavioral RED.
Task 3: RED observed: 7/7 search methods failed against pending empty renderer; implementation now consumes the original full-file events.
Task3 debugging: CRLF service line produces Html(comment) plus separate Html(newline); per-event prefix filtering leaked newline. Dedicated crlf regression observed RED. Renderer now excludes validated whole-line metadata ranges, preserving literal code examples.
Task3 macOS: 27 integration tests PASS, fmt/clippy PASS. Source and golden hashes will be pinned in verification.json; Docker build running.
Ruling: commit Task3 implementation/provisional evidence before whole-branch review, then finalize docs/status after findings — review-package needs committed HEAD while plan Task3 placed review before its commit — cost if wrong: provisional report remains explicit until finalization; no merge/push.
Task 3: complete (commits 524231f..79cc52d, tests: cargo test --locked → test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
Final reviewer: 2 Important, no Critical/Minor; both reproduced locally before fixes.
Final: fixed raw HTML metadata false positive — raw_html_content_does_not_declare_metadata RED→GREEN, 10 element tests PASS; suite30/30 PASS after both fixes.
Final: fixed image alt link false edge — image_alt_links_do_not_declare_edges RED→GREEN, wrapping-image link guard PASS; suite30/30 PASS, fmt/clippy PASS.
Final: Ruling: CR-only EOL remains unpromised — spec/tests guarantee LF/CRLF — cost if wrong: CR-only documents may need EOL conversion before indexing.
Final: Ruling: additional heading attributes/container headings follow CommonMark plus X4 anchor/standalone-metadata checks — no extra restrictive grammar was agreed — cost if wrong: later X4 clarification requires new fixtures/restrictions.
Final: Ruling: FS target/symlink checks, includes cycles and Git/SQL/CLI/model integration remain subsequent tasks — parser is pure and cannot establish them — cost if wrong: callers must not treat parse success as those guarantees.
Final: Ruling: large-scale performance and x86_64 remain unverified — acceptance here is scoped parser ARM64 proof — cost if wrong: later measurements may require changes.
Final: Ruling: Linux evidence validated by executor/hash, not independently rerun by reviewer — repeatable full offline run plus receipts is sufficient here — cost if wrong: no independent Linux reproduction.
Final: deferred minors: none.
Final verification: fixed source commit3aff95f; 30/30 integration tests, fmt/clippy PASS on macOS ARM64 and Linux glibc ARM64 --network none, final image677a8165. No second review, no deferred minors. Card/roadmap acceptances completed locally; integration remains user decision.
