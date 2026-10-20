# TK-1 — independent review

Date: 2026-10-21. Reviewer: separate read-only agent (`review_tk1`), with fresh context. Scope: new `tests/storage-build/`, implementation plan and TK-1 evidence; copied baseline documents excluded.

One material finding (P2): publishing the shared `.build/vec1.c` through direct `write_bytes` lets concurrent first-run readers observe incomplete source; interruption can leave corrupt cached bytes. The reviewer reproduced a legitimate reader failing SHA-256 during the other writer's partial write.

Resolution: write the already verified bytes into a unique temporary directory beside the destination, then atomically replace the cache path. Added `test_partial_write_is_not_visible_to_cache_readers`; its controlled partial-write fixture failed before the change ([RED](source-publication-red.txt)) and passed after it, including both final platform suites. All 10 Rust and 4 Python tests, formatting and Clippy passed on both platforms after the fix. No material findings remain open; no minor findings were deferred. This is verification by the implementer after the review fix, not a second independent review.

The reviewer found the connection policy, missing-extension failure, explicit/RAII rollback oracles and native-library isolation sound. Earlier manifests predated the explicit unsafe signature; both final manifests now match the tested sources.

Declined to judge: copied design baseline, production storage, ATTACH/config reset, physical durability, x86-64 and performance. These remain outside TK-1; no completeness claims about them were added.
