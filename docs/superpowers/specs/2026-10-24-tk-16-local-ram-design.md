# TK-16 — Local RAM assessment

Status: user directed “ok just assess ram usage” on 2026-10-24, opting out of
further design questions. This is a local assessment, not 4 GB hardware acceptance.
Base main@ccb15f0; branch tk-16 (renamed from codex/tk-16); isolated managed worktree tk-16/letopis.

## Intent and constraints

Use the completed TK-15 corpus/model and existing release CLI to measure RAM on
the available Mac (48 GiB, Mac16,7, 14 CPUs). Keep the original populated corpus,
stores, binary pins and receipts intact. Product architecture and synchronous
runtime do not change. Sources: [TK-16](../../zkit/letopis/roadmap/epics/EP-7-early-validation/tasks/TK-16-early-memory.md),
[resource protocols](../../zkit/letopis/resource-math.md),
[architecture boundaries](../../zkit/letopis/architecture.md#program-structure),
[TK-15 evidence](../../zkit/letopis/checks/TK-15/report.md).

## Measurement approach — author choices

- New ignored operational scripts/receipts under .artifacts/tk16; no reusable
  product profiler, dependency or runtime is needed for this assessment.
- Copy the populated repository while holding its existing index/embed locks
  through read-only descriptors; byte-verify original and copied databases.
  Independently generate the identical input-only fixture for a fresh backlog.
- Reuse pinned release product/observer artifacts; all 146 baseline inputs match
  the just-verified main snapshot. No repeated unchanged root suite or full100k drain.
- Use actual search/load/drain commands. Capture populated coverage before/after;
  record cold/relocated and warm search separately. Profile initial backlog load,
  then a bounded45-second foreground drain with a concurrent search. Only the
  owned drain is interrupted; incomplete backlog is expected and remains explicit.
- Sample process trees with ps and macOS footprint; record native JSON in bytes,
  sampled physical charges, group footprint and RSS diagnostics separately.
  Include CLI/Git and the Ollama wrapper/server/model runners. Record test
  orchestration/observer overhead separately. RSS is not unique total RAM;
  size_vram is metadata and must not be added to unified-memory footprint.
- Calibrate the measurement with an owned process touching64MiB. Retain failed
  samples and model/process inventories; never substitute missing data with zero.
- Wait for unrelated models to expire naturally. Never unload/kill shared
  provider processes. Flag any concurrent unrelated model as contamination.

Apple explains the distinction between footprint, resident/clean pages and
unified CPU/GPU objects: [WWDC22](https://developer.apple.com/videos/play/wwdc2022/10106/).
Native tool help and raw receipts record the actual installed metric schema.

## Verdict and handoff

Report measured maxima as observations on this host, not an absolute unsampled
peak, complete OS-cache attribution or proof on a 4 GB laptop. The original
1,000,000,000-byte budget remains unchanged. TK-16 hardware acceptance and G-1
remain not assessed without the required equipment and complete accounting.
No merge/push/GitHub closure is authorized by starting this assessment.

## Observed method limitation

Both short concurrent search CLIs finished between process snapshots; overlap RAM
rows therefore measure sampled drain/provider only. Independent query time maximum
RSS is retained separately and cannot be added as a simultaneous peak. The local
report discloses this omission; complete concurrent accounting remains unassessed.

## Follow-up correction

User subsequently required search to work during drain. The
[correction plan](../plans/2026-10-24-tk-16-search-contention-fix.md) and
[report](../../zkit/letopis/checks/TK-16/report.md#follow-up-concurrent-searchdrain-correction)
record a verified5-second wait scoped to the independent vector writer. The
original RAM captures stay pinned to their original binary; this software fix
does not establish4GB acceptance or new full-memory maxima.
