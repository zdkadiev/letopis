TK-6 implementation and verification are complete in the local `TK-6` worktree based on `cc82a8f`. Changes remain uncommitted; this comment does not claim they have been merged or pushed.

- D-96 stack: blocking ureq 3.4.2, explicit rustls-platform-verifier/system trust, 60s attempt timeout, no redirects/environment proxies/Tokio/reqwest/application retries.
- Native Ollama and OpenAI-compatible request/response validation, finite f32/count/dimensions/index checks, strict config root with open parameters and allowlisted forwarding, safe local errors.
- Foreground signal-hook cancelled JSON/exit1 and stdout once-guard; detached drain's default signal death tested separately.
- Final macOS ARM64 and Linux glibc ARM64 runners PASS: 8 Rust and 19 Python test methods per platform, including actual 60s timeout. Linux runs with `--network none`. All 10 macOS / 11 Linux compiling negative controls detected their named behavioral failures and restored source hashes.
- Disposable Linux system CA before/after plus hostname mismatch and WebPki mutation prove system-trust selection there. Separate macOS public HTTPS probe PASS; no host Keychain/system-store edits, credentials, knowledge or inference.
- One independent review found cancelled JSON/exit0 and an overly strict success/exit1 race oracle. Both fixed with RED→GREEN. A deterministic gate after completed HTTP success proves the exact race on both platforms; a dedicated exit-status mutation detects removal of the fix. No deferred minors or second review.
- TK-1…TK-5 current covered source hashes match ten committed PASS manifests; the 31 pilot-protocol tests also pass freshly. G-0 is recorded as continue to TK-7.

Artifacts: `tests/transport/`, `docs/zkit/letopis/checks/TK-6/{report.md,review.md,macos-arm64/,linux-arm64/}`, updated Superpowers spec/plan and roadmap/card, plus Engram records. Historical FAIL/PASS receipts are preserved, including the missing-rustfmt Docker setup failure and the earlier insufficient race-control gate.

Scope remains synthetic transport: no model inference/utility pilot, guaranteed Ollama minimum, arbitrary-server conformance, x86_64 proof, retry/storage lifecycle or full SQL/flock/git command cancellation. Installed Ollama client reports 0.40.7; it was not exercised for embeddings. Later owners remain TK-9/TK-13/TK-27/TK-29/TK-41/TK-43.
