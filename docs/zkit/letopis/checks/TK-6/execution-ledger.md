# SDD ledger — plan: docs/superpowers/plans/2026-10-21-tk-6-embedding-transport.md

User Go on accepts rev3 plan for implementation. Baseline cc82a8f, branch TK-6. No commit/merge/push authorized for TK-6.

Pre-flight: Task 1 → 2: Config/prepare/decode/closed errors match the Client input; add explicit Config::parse for strict JSON boundary.
Pre-flight: Task 2 → 3: runner consumes actual CLI/process fixtures, no mock substitutes.
Ruling: omit optional injected-CA helper — system verifier tested in disposable Linux trust store and separate macOS public HTTPS probe; fewer test-only code paths.
Ruling: normal/control output shares a Mutex once-guard held through write/flush; signal thread is std thread only and never performs HTTP.

Task 1: in progress.
Task 1: complete (commits cc82a8f..cc82a8f, tests: cargo test --locked --manifest-path tests/transport/Cargo.toml --test protocol → test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)

Task 1: behavioral RED (4 failures), GREEN (7), extra root-prefix regression RED→GREEN (8). Interface parts() exposes adapter-only wire data; Debug is redacted. Exact versions in Cargo.lock.
Task 2: real-wire CLI stub RED for both API variants, now implementing blocking adapter and signal handler.
Ruling: body with_config/read_to_vec keeps X-2 without inventing an HTTP size limit; CPU/memory exhaustion remains explicit future resilience risk, timeout bounds network I/O only.

Task 2: first integration run had 5 test-oracle failures: HTTP headers were incorrectly case-sensitive; proxy fixture returned native payload to an OpenAI client. Observed actual request confirmed authorization lowercase and valid compatible vectors; corrected fixture response and case-insensitive oracle, strengthened trap to observe CONNECT. No adapter fix needed.
Task 2: complete (commits cc82a8f..cc82a8f, tests: /Library/Frameworks/Python.framework/Versions/3.12/bin/python3 -B -m unittest discover -s tests/transport -p test_transport.py -v → OK)

Task 2: Linux ARM64 --network none system trust proof PASS: untrusted before CA install, success after disposable-container system CA install, hostname mismatch rejected, container CA removed afterward. Separate macOS public GET https://example.com/ through same Agent factory PASS; no input/key/inference or host Keychain edit.
Task 3: runner regressions RED (6 assertion failures across 3 methods) → GREEN (3 methods). Mutation proof discovered a weak oracle: ureq already refuses POST body replay for 307/308, so changing redirect policy alone did not trip a 307-only check. Added 301/302/303 and observed GET as well as POST/CONNECT; redirect mutation now detected. Nine macOS compiling controls all DETECTED; original source bytes unchanged.
Ruling: system-trust mutation applies on Linux only, where disposable system CA distinguishes PlatformVerifier from Mozilla roots; no host trust mutation on macOS. Both platforms still exercise normal shared TLS adapter and untrusted rejection.

Task 3: first complete macOS receipt PASS. Linux receipt FAIL because pinned base image lacks rustfmt; this is an environment/setup failure, not a transport failure. Both receipts preserved byte-for-byte under checks/TK-6/before-tooling-fix.
Task 3: Ruling: install rustfmt/clippy while building the pinned Docker image before --network none — required checks cannot be silently skipped.
Task 3: Ruling: move full Python suite after disposable controls and run runner regression tests first — satisfies post-mutation whole-suite GREEN without a duplicate 60-second test. Final receipts will use new source snapshot.

Final review: Ruling: keep user-requested uncommitted delivery; review-package has empty base..HEAD. Supply actual untracked files and immutable source SHA256 snapshot to fresh reviewer instead of committing solely for tooling.
Task 3: complete (commits cc82a8f..cc82a8f, tests: /Library/Frameworks/Python.framework/Versions/3.12/bin/python3 -B .superpowers/sdd/2026-10-21-tk-6-embedding-transport/verify-evidence.py → linux-arm64 PASS; current sources, receipt hashes, committed prior proofs and 10 compiling controls verified)

Task 3: full macOS and Linux glibc ARM64 runners PASS against current source snapshot; 8 Rust + 18 Python methods on each platform, 9/10 compiling controls DETECTED. Task-done validates immutable receipts/hashes after actual full run commands, avoiding another redundant 60s execution. Positive macOS TLS receipt separate PASS; prior 10 platform manifests remain committed and current. Fresh final review next; G-0 not closed before review.

Final review: reviewer /root/review_tk6_final (gpt-6-astra/high, fresh context) returned two Important findings, no Critical/Minor. Both accepted by user effect: cancelled JSON with exit0 breaks X-1; a race oracle rejecting permitted success/exit1 makes acceptance flaky.
Final: fixed cancelled/exit0 — test_cancelled_output_wins_exit_status_when_http_finishes_later deterministic RED→GREEN. Added fixture signal-post-write gate and response release to force the exact interleaving, without changing default behavior. Main now exits1 if it lost stdout guard.
Final: fixed success/exit1 oracle — extracted the shared race outcome assertion and exercised it via existing post-output gate, RED→GREEN. Cancelled still requires exit1, success accepts exit0/1, one complete JSON/no stderr preserved.
Final: Ruling: add cancelled-exit compiling mutation targeting the exact regression — makes the fixed exit-status guard falsifiable; platform controls become 10/11.
Final: Ruling: retain network-only deadline/oversized-body limitation — no accepted body budget or compute executor; cost is unbounded hostile-body memory, still outside narrow proof.
Final: Ruling: blocked stdout and SQL/flock/git cancellation remain TK-29 — bounded fixture output establishes only tested transport points; cost is no full CLI guarantee yet.
Final: Ruling: no remote-compute cancellation claim — local exit cannot revoke provider work; remote cost may continue.
Final: Ruling: real inference/model quality/arbitrary provider conformance/retries/storage/FTS stay TK-13/TK-27/TK-41 — synthetic endpoints cannot prove them.
Final: Ruling: macOS corporate Keychain installation/x86_64 remain unproved and labeled — Linux CA distinction plus narrow public macOS TLS result retained; no host trust edits.
Final: Ruling: do not invent further config/integer/f32/duplicate-member/native-model-equality constraints without demonstrated contract need — cost is documented existing accepted boundary only.
Final: Ruling: URL/credentials/redaction/redirects/proxies reviewed without further actionable finding — keep implementation and real trap evidence; no speculative tightening.
Final: Ruling: --network none proves local synthetic transport only — no full real-model offline drill claim.
Final: Ruling: historical FAIL/PASS receipts remain unchanged under before-tooling-fix/before-review-fix; update status only after fresh fixed-code runs.
Final: Ruling: full Letopis design audit remains excluded by explicit user scope.
Final: deferred minors: none. Fresh whole suites/receipts must pass before completion; no second reviewer.

Final fix pass still in progress: first fixed-code macOS full run PASS (10 controls), Linux rejected cancelled-exit negative control because mutant unexpectedly exited1. A held pending HTTP request does not guarantee the main completion is successful on Linux after a signal; this could mask the missing guard. No false DETECTED accepted. Both receipts preserved under before-deterministic-gate.
Final: Ruling: strengthen deterministic fixture gate to after successful HTTP completion but before normal output; readiness explicitly records success. Signal thread writes cancelled and waits; release main only after that JSON. This forces the intended successful-main/cancelled-output ordering independently of platform I/O interruption. Guard and process architecture unchanged. New target test is test_cancelled_output_wins_exit_status_over_completed_http_success. Compile mutation must now fail the exact oracle on both platforms before full green receipts.

Final: fixed both Important findings — deterministic RED→GREEN; final complete fixed-code suites 8 Rust +19 Python per platform PASS; 10 macOS/11 Linux compiling controls DETECTED; actual60s timeout included. TLS preflight/final binary hashes PASS. No deferred minors.
Final: G-0 continue→TK-7, task/card/EP-8/roadmap/spec/plan updated with full report/review evidence. GitHub issue6 report posted; issue remains open pending separate integration instruction.
Final integration: keep branch TK-6 and worktree as-is under accepted uncommitted-delivery plan; no new permission/menu needed, no commit/merge/push. Ledger is archived to checks/TK-6 before this plan scratch workspace is removed.
