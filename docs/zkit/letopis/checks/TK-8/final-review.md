# Final whole-branch review — 2a7e8db..a6af7d7
Reviewer /root/review_tk8_final, gpt-6-astra/high, read-only. Not ready to merge. NoCritical/Minor; oneImportant.

## Important: retention with external alternates
refs.rs:26 and objects.rs:66 resolve candidate objects through objects/info/alternates, including ordinary git clone --shared. A borrower service ref does not retain donor objects. Both SHA1/SHA256 public-API reproduction: create_tree reuses donor blob, pin succeeds, donor GC/prune removes blob, subsequent read InternalError while borrower ref remains exact. hash-object -w alone does not materialize alternate objects. Fix: explicit refusal of effective alternates or materialize locally; regression must cover donor GC.
Reproducer: /tmp/tk8-final-review-probe/reproduce.py plus public Rust harness. Checkout unchanged.

Strengths: physical architecture, raw identity/type/path handling, process IO/error handling, candidate closure/direct-ref retention and real Git/GC/control tests. All41currentSource/14newEvidence/30historicalEvidence hashes checked. Task-card policy changes reviewed and links resolve; no retrospective claims.

## Declined behaviors and controller disposition
Fullcapture/InspectedElement/sourcecomparison/SQL/CLI; atomiclivecapture; powerloss/TK38; actualGit2.29; x86_64; cancellation/TK29; resource/throughput/extreme-depth/threadallocation; hostile sameUID mutation; multi-worktree product semantics; repository-wide fsck; historical-harness reruns: match explicit spec/plan limits; retain disclosures, no implied PASS.
NonUTF8 checkout roots: current discovery safely rejects; explicit controller ruling below.
Administrative final acceptance documents remain pending gates.
