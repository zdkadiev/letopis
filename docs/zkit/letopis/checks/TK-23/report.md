# TK-23 — incremental metadata-only traversal

Implementation/tests/report ready for review; **required Linux native acceptance is blocked by infrastructure**. **Task 4 independent review and final whole-branch review pending**; task-card completion and integration remain controller/user decisions.

## Revisions and scope

Product/test checkpoint: `e9a9cd4a4c9d6807b0a75d79022dc8f838796e78`, parent `b8b89f6fe2d1a83d6f50041ceae5735b86264ec3`. Task 4 adds regressions and moves existing inline binding tests into private files; no product behavior defect or production fix was found. Tasks 1–3 supply storage/BFS/CLI implementation. Historical baseline 620/0/5 and task-specific earlier receipts remain historical.

Task 4 files: `src/service/graph/tests.rs`, `src/service/traversal.rs`, private `src/service/traversal/tests{.rs,/bindings.rs,/scoped.rs,/traced_storage.rs}`, `tests/traversal.rs`. The private split keeps bindings.rs at 244 lines, scoped.rs at 442 and traced_storage.rs at 155: the latter solely delegates real associated storage capabilities and owns read/page recording plus the after-pin hook. The controller confirmed this cohesive child is within the approved allowance. Public interfaces, dependency/runtime set and DDL did not change in Task 4. The branch's cache-v2/index changes from Task 1 remain in final acceptance.

## Behavior and independent oracles

- Publication after the first real neighbor page replaces a→b with a→c and b→old.rs/S1 with b→new.rs/S2, deletes old.rs's final current membership and changes b's coordinates from 4–8 to 20–30. The original scoped read returns literal `[b.md, old.rs]`, original edge triples, coordinates 4–8, revision 1 and S1/old.rs provenance. c/new.rs/S2 never enter the recorded observations. Live stale/source flags are checked separately.
- Readiness fixture records exactly refresh→readiness→begin_read once, accepts Missing/Invalid configuration, and panics on query inference, vector/text ranking or whole-list access.
- Lazy degree 64 versus 100,000,000 uses the same three selected literal identities, one retained edge, one page and no child expansion; node cap stops mid-buffer despite the earlier independent edge cap. GraphRead exposes no metadata/list method. The reused integration fixture corrupts unselected metadata and remains successful until that node is selected.
- 70 outgoing bindings produce page/observation events `page:0, observe:32, page:32, observe:32, page:64, observe:6, page:70`; each binding belongs exactly to its once-observed page, provenance request size never exceeds 32, and flags remain a deduplicated finite union. Page-64 failure follows two completed observations. Empty bindings still validate basis once.
- Independent late graph-page/source/provenance failures return safe InternalError without partial links, retain a private cause, release the real read transaction while the service remains alive, allow real WAL TRUNCATE checkpoint and subsequent real cache publication. Success and depth-zero empty results release likewise. Real nonregular source is an operational error while missing local source yields SourceNotFound; existing unsafe symlink coverage is reused.

## Controls, classification and restoration

The new coverage initially passes existing correct ownership; it is **already-green regression coverage**, not an invented feature RED. The first development run had one fixture bug (provenance failure was not reached); this was corrected in the fixture, never classified as a product defect/RED.

Ten production controls against unchanged final fixture bytes produce runtime failures (exit 101): reopen after concurrent publication; move readiness behind pin; swallow late graph page; swallow provenance; swallow source; collect all binding pages before pagewise observations; overrun node cap; swallow late binding page; retain successful reader; hide unsafe source error. Driver retains patches and per-control logs, asserts fixture hashes before each execution, asserts actual mutated source at execution, and restores production bytes after each case and in finally. Runtime assertions establish the result/order/error/bounded-observation failures; endpoint hashes only establish endpoint consistency and are not offered as independent evidence of no transient historical edit.

Evidence: `.artifacts/tk23/task4-20261026/{controls.py,controls.json,controls-driver.log,*.patch,*.log}`. Restored scoped tests pass 8/0/0. Focused preflight library + 16 relevant integration roots passes 380/0/3 across 17 roots; query-plan regressions, fmt, all-target Clippy and strict Rustdoc pass. Relevant integration roots: graph_read, traversal, cli_traversal, source_reading/lifecycle/addresses/projection, provenance_observation/indexing/commands/revisions, search_consistency/contention/filter_fallback/filters, knowledge_search.

## Final platform acceptance

| Platform | Full runner verdict | Captured debug roots | Captured release roots | Other evidence |
| --- | --- | --- | --- | --- |
| macOS arm64 host | PASS, exit 0 | 684 passed / 0 failed / 5 ignored, 48 executables | 684 / 0 / 5, 48 executables | Python runner 10/0; explicit deadline 1/0; native 1/0; DDL/standalone/linkage PASS |
| Linux arm64 Docker | **FAIL / infrastructure blocked**, exit 1 | 684 / 0 / 5, 48 executables in preserved snapshot | **Partial snapshot only:** 578 / 0 / 4, 39 executables | Final inner manifest/native/standalone/linkage receipts unavailable; no full platform acceptance |

Commands (repository root): `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target RUST_TEST_THREADS=2 /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk23/final-host-task4-20261026-01`; Linux uses `--platform linux --evidence .artifacts/tk23/final-linux-task4-20261026-01`. The retained test-only Docker argv wrapper injects `--env RUST_TEST_THREADS=2` only into `docker run`, records exact executed argv and delegates other commands unchanged. Host uses the shared target above; Docker uses `/repo/target`. Runtime container has `--network none`; preparation used existing Dockerfile `cargo fetch --locked`.

Host captures exact `make verify V=1` exit 0, canonical cache/vector DDL parity (project seed excluded), 10 runner Python tests, strict Rustdoc, explicit real 60-second ignored deadline probe, native SQLite 3.53.2 / vec1 0.8 (NEON, multi-threaded), copied standalone FTS/shared native vector/exact bytes/reopen/revision-1 check, and `otool -L` with no shared SQLite/vec1 dependency. No redundant standalone full make verify was run.

Linux image build succeeded for arm64 and Docker server 29.9.2 was checked live. One in-progress `/receipt/rust-check.log` copy proves complete debug counts and only 39 completed release roots; this snapshot is preserved as `.artifacts/tk23/task4-20261026/linux-in-progress.log`. Later live inspection reached the explicit deadline probe. Final traceback reached the filtered native capability cargo command (exit 101), but its detailed log was not recovered. The inner runner then failed writing `/receipt/manifest.json` with `OSError: [Errno 30] Read-only file system`; outer copy failed `chown ... overlay2/.../work/work: read-only file system`, and owned-container cleanup failed `unlinkat ... read-only file system`. Thus later stage reachability is recorded, not promoted to a fully captured release/deadline/native PASS. The final inner manifest, native log, standalone and linkage results remain unavailable.

Diagnostics confirm a dead owned container and read-only Docker overlay2 backing filesystem. Host Data volume had 1.8Gi available with 100% displayed capacity; storage pressure is a plausible contributor, **not a proved root cause**. One read-only export attempt was rejected because the container is Dead. No daemon restart, reset, broad prune, unrelated cleanup or repeated full run was attempted. The owned dead container/image may remain because cleanup itself failed. Required Linux native/standalone acceptance stays open until Docker is writable and a fresh runner succeeds.

Root counts exclude nested child/doc summaries. Host's five ignored entries per profile are three private subprocess entry points (invoked by parent tests), real deadline (executed separately) and opt-in existing-corpus probe (not run); separate probes are not added to root counts. Linux partial counts are not a final total.

Receipts: `.artifacts/tk23/final-host-task4-20261026-01/manifest.json` (PASS); `.artifacts/tk23/final-linux-task4-20261026-01/manifest.json` (FAIL, preserved); `.artifacts/tk23/task4-20261026/{final-host-root-counts.json,final-linux_partial-root-counts.json,infrastructure-diagnostics.log,infrastructure-diagnostics.json,docker-export-recovery.json,docker-invocations.jsonl}`. Both outer maps equal the frozen 319-file map; current endpoint hashes match. Container input-map comparison is unavailable and is not claimed.

## Input identity

Both required native runners were attempted once on frozen checkpoint e9a9cd4; host passed and Linux failed as detailed above. Product/test/build/DDL input map has 319 entries, including newly created test files and clippy.toml. Canonical sorted-JSON map SHA-256: `143fd382892be2c3adfbd3f259b438d4364b22e2a03e987185903d0567e3ac6a`. Exact maps are retained in frozen-inputs.json and native manifests. Both outer maps and all current endpoint files match before documentation edits. The inner Linux map could not be recovered. Later documentation commits do not relabel these source receipts.

## Architecture review and memory limits

Domain Graph owns ordered domain values only; service/graph.rs owns BFS, compact discovery IDs/FIFO, selected metadata-free identities and optional deduplicated edges. GraphRead/GraphTraversal are object-safe; only Storage/FileSystem associated-type generics remain in the orchestrator. SourceReader receives the same read handle. lib.rs constructs collaborators, CLI invokes Traverser, SQL remains in storage. New concrete SQLite/filesystem references occur only inside private test code. No product hook, public fixture API, forwarding layer or speculative abstraction was introduced.

Neighbor streams use per-direction strict (type, neighbor ID) keysets with actual endpoint joins/filtering before LIMIT. Query-plan evidence contains 36 stream plans and one basis plan, covering both directions, all modes, no/single/multiple type filters, first/cursor pages and the narrow basis query: directional covering indexes, no OFFSET/metadata load or temporary ordering B-tree. Existing both-direction ties/back-edge/orientation/alternative-path/transport/old-cache/vector/warm-refresh regressions are exercised by final root suites.

BFS state is proportional to starts and selected results/optional retained edges, plus at most 64 buffered neighbor records. This is not a fixed-RSS guarantee for the complete CLI call. Existing candidate/file-state/root sets, first/changed publication projections, file parsing, per-page provenance diagnostic sets and JSON result buffers remain separate costs. Live observations are not an atomic snapshot across files.

## Cache rebuild and remaining gates

Cache schema 2 recognizes directional indexes and narrow source roots; vectors schema remains 1. Incompatible old cache fails safely before vectors are opened. Explicit rebuild requires all commands/drains stopped and handles/locks released; remove **only derived cache.db and existing quiescent cache sidecars**, preserve vectors.db/sidecars, config, persistent lock identities, Markdown and Git, then `letopis check` with `{}`. No user database was deleted: all tests use temporary fixtures. See the development guide's existing TK-20/TK-23 rebuild procedure.

Task 4 implementer self-review is complete; Linux native/standalone acceptance remains open; independent Task 4 review and final whole-branch review are separate pending controller gates. TK-24/TK-28, paid/model/pilot/resource/G-1/G-2 verdicts and exhaustive new cancellation/platform drills remain deferred. No merge/push/GitHub changes or Engram writes were performed by this delegated implementation task.

See the [development guide](../../../../development.md), [approved specification](../../../../superpowers/specs/2026-10-25-tk-23-traversal-design.md) and [plan](../../../../superpowers/plans/2026-10-25-tk-23-traversal.md). No captured receipt is relabeled to a later documentation commit.
