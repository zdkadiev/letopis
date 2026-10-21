# TK-3 independent review and fix verification

2026-10-21. Reviewer `/root/review_tk3`, fresh context, gpt-6-astra/high, read-only. Base/HEAD `2ea7e581bff5388f8199bd996669aff5d939426f`; review explicitly included new untracked files, plan/spec, source inventory, both platform receipts and five Review Focus cases. No implementation agent was used. Reviewer experiments ran in a separate temporary checkout; the worktree remained untouched.

## Initial verdict: With fixes

Critical: none. Important: three. Minor: none.

1. Late-error isolation oracle was vacuous: the post-reset failure table was empty, so removing only record_failure's identity guard still passed all 11 tests. Fix: reserve the SAME alpha hash in the new configuration and set a distinguishable `new-config-sentinel` error before releasing the old response; compare the entire vector database afterward.
2. Stored provider-vector bytes were unverified: replacing every stored vector with all-ones floats still passed all 11 tests. Fix: independently query vector_keys JOIN vector_index and compare literal alpha `[1,0,0]` / bravo `[0,1,0]` bytes after both committed responses, including the shared-alpha case.
3. Runner timeout killed only its direct child: a reproduced spawned sleep process survived. Fix: execute commands in their own POSIX sessions, kill their command process group on timeout/interrupt, then reap. Two real subprocess tests establish readiness through a socket and require EOF after cleanup.

Reviewer reproductions are retained under `review-fixes/reviewer-*`. New narrow late-failure-guard and same-dimension-byte-corruption mutations fail at invariant assertions (exit 101), then source is restored byte-for-byte. New process tests first failed twice with `runner left descendant alive`, then passed both timeout and SIGINT cases after the process-group fix. See `review-fixes/manifest.json`, `runner-red.log`, `runner-green.log`, and `review-rust-green.log`.

One fix pass, followed by complete refreshed platform runs. No second independent review is claimed. Final acceptance requires matching source hashes and passing final receipts; see `final-verification.json`.

## Declined-to-judge items and parent rulings

- Concurrent production orphan cleanup/publication remains unproved by snapshot cleanup; retain the original ruling, future production lifecycle work must settle it. Cost if wrongly generalized: a referenced vector/failure may be reclaimed.
- Real provider transport/authentication/cancellation/retry timing remains TK-6/runtime work; deterministic outputs prove identity/budget only. Cost: transport behavior can still fail.
- Production drain ownership, simultaneous workers, launch/flock remains TK-4 and production lifecycle. Cost: concurrency outside these schedules remains unproved.
- Full Markdown, arbitrary in-place writes and general JCS/config validation remain the approved bounded-fixture boundary. Cost: full input handling remains unproved.
- Product ranking, quality and performance are not inferred from ID-sorted fixture results. Cost: retrieval quality/capacity remain unmeasured.
- Physical power-loss durability remains TK-38. Cost: process-death evidence cannot establish device durability.
- Corrupt-database repair, production migrations and arbitrary invalid public API inputs remain production-adapter work. Cost: those recovery/input guarantees remain unproved.
- x86-64 remains untested; both receipts are ARM64. Cost: portability beyond the tested platforms remains unproved.
- Final status/roadmap acceptance is parent-owned and occurs only after the refreshed checks; G-0 remains open. Cost: prematurely claiming product readiness would misstate this proof.
