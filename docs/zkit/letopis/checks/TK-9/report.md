# TK-9 storage verification

Status: complete, independently reviewed and merged into local `main` as `ec96a8e`. GitHub issue #9 is closed. Push remains pending.

Current verification source: `c93a2f9d795da1ea211e3ef5e7cdb939447aa255`. [Final results and source checksums](verification.json) record actual source identities, platform details and local raw-receipt locations. Later documentation commits are not claimed as executed test revisions.

| Platform | Rust tests | Python tests | Result |
|---|---:|---:|---|
| macOS ARM64 | 106 | 8 | PASS |
| Source-built Linux glibc ARM64, network disabled | 106 | 8 | PASS |

Both platforms passed formatting, compiler, Clippy, strict Rustdoc, canonical-DDL parity, native capability assertions, copied executable behavior and linkage inspection. All 53 pre-storage scenarios remain. The additional Python regression proves nested `target/`, `.build/` and `__pycache__/` directories are excluded from inventories. The host inventory now contains 182 source inputs; Linux's 77 source inputs and their SHA-256 values are retained in the final summary.

| Acceptance criterion | Verification |
|---|---|
| Schema, graph/source/files/contentless FTS and atomic units | Bootstrap/cache/vector/failure tests cover stable row IDs, declared dangling targets, source projections, CHECK/FK failures, unsigned overflow, drop rollback and caught-error poisoning. |
| WAL/FULL/FK and macOS fullfsync | Actual applicable connection PRAGMAs are read back and asserted for both databases. |
| Statically linked vec1 | Pinned original C digest, native round-trip/search, forbidden SQL loading, copied executable and linkage checks. |
| No copied raw document bodies | Metadata-only interfaces, schema inspection and contentless FTS tests. Tokens/postings remain expected. |
| Hexagonal responsibility modules | Service-owned interfaces, separate concrete implementation files, inward imports and actual lib.rs constructors; see [review outcome](review.md). |

Model-reset tests keep populated old/new read snapshots distinct, reject stale identities and join vectors only through current text hashes. Failed vector/key writes and reset operations roll back together. Attempt counts survive restart and exhaustion; query errors persist until explicitly cleared after success. Safe error displays retain internal causes without exposing payloads.

SQLite 3.53.2 and vec1 `version 0.8 (NEON, multi-threaded)` are observed values. Assertions require SQLite >=3.51.3, the vec1 0.8 prefix, actual capabilities and the connection policy.

Two native negative controls passed at source `60fc1f0`: altered C fails checksum verification before compilation; disabled registration compiles successfully, then fails the native runtime oracle. Native source/build inputs have not changed since those controls. Deliberate canonical-DDL drift is rejected by the retained Python test. Initial Linux COPY failure and all four resolved review findings remain in historical evidence.

## Evidence retention

Only this report, the review outcome and one final JSON summary are tracked. All 167 historical receipts were archived byte-for-byte before removal from the current tree. The ignored local archive is `.artifacts/tk-9/raw-evidence-4813a3f.tar.gz`; its SHA-256 and the historical Git revision are recorded in `verification.json`. The original files also remain retrievable from Git commit `4813a3fd1e426012eb80ac120bc5d309367d4dc8`. Raw current runs are under `.artifacts/tk-9/cleanup-host` and `.artifacts/tk-9/cleanup-linux-verified`; CI runs should retain equivalent output as job artifacts.

Reproduce using the commands in `tests/storage/README.md`, with fresh ignored evidence directories. No commit history was rewritten.

Unclaimed: physical power loss, x86_64, minimum Git execution, capacity/model-quality gates, full CLI/publication/ranking/provider/drain, and execution of historical proof harnesses.
