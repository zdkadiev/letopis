# Search optimization execution ledger

Spec: [search optimization design](../specs/2026-11-06-search-optimization-design.md).

1. Pin current source/binary/corpus and preserve existing dirty tree.
2. Run four independent scoped investigations in owned source/target directories.
3. Review methods and contract counterexamples; retain safe candidate patches.
4. After heavy agent activity stops, screen candidates serially on the full
   populated 100k fixture using exact request/result/cardinality oracles.
5. Integrate the chosen method/combination, mutation-check meaningful tests,
   run make verify on the actual final root and pin the release.
6. Measure three alternating baseline/candidate pairs with fresh CLI processes,
   capture genuine Samply stacks, and independently review code/data.
7. Save a new report/evidence index and recheck source, corpus and historical
   artifact hashes. Preserve root edits and the unrelated issue-97 worktree.

## Progress

- Baseline source106 hashes and binary match captured3d820de. Initial root diff is
  saved in `.artifacts/issue-65/search-optimization-2026-11-06/working-tree-before.patch`.
- Four agents investigated parse-memo, parser-cpu, sql-cost and ranking-contracts
  in owned directories before root integration.
- Ranking-first shortcuts rejected: beyond-limit malformed-candidate test and
  full eligible-channel rank/error/freshness contracts. Three literal probes pass;
  lost-overlap mutation is detected.
- SQL-only probe shows indexed metadata/edge lookups with zero fullscan steps;
  full native revision/metadata owners are <0.5% CPU each. No SQL patch retained.
- Parser cursor microbench: 1.28230 → 0.81233 s median over 9×2000 parses,
  identical full projection. Line-count byte visits 1799970 → 59999 in a literal
  30×2000-byte fixture. These are parser-only results, not full-search speedups.
- Memo candidate has RED→GREEN work-count cases and fresh-read preservation;
  transition/budget/mutation verification completed in its isolated clone.
- Both independent candidates passed make verify in owned clones. Memo: eight
  new cases and ten viable manual mutants, restored final source; parser: one
  new coordinate case and caught cursor-reset mutation, plus all Markdown suites.
- Whole combined-candidate review found no actionable findings. The existing
  MarkdownCodec contract already documents deterministic/no-I/O results for the
  same path/bytes; no public contract edit was needed.
- Combined 36 search unit tests and relevant integration/Markdown suites passed.
  One initial invocation misspelled markdown_link_lifecycle as markdown_lifecycle;
  it ran no tests, its command error is retained separately, corrected run passed.
- Successful conditioning CLI call116.616 s was retained and independently
  revalidated after a measurement-wrapper exclusive-file write collision. Driver
  now uses separate immutable raw and final receipts; no product failure occurred.
- Serial full100k screens: baseline117.019962 s, parser91.075066 s,
  memo62.733121 s, combined61.145912 s. All result/cardinality/vector oracles valid;
  these are single screening samples, not final paired performance evidence.
- Selected both simple compatible changes. Integrated only service/search.rs,
  markdown/scan.rs and tests/markdown_elements.rs after verifying untouched root
  baseline bytes. Existing user diff was preserved.
- Actual-root `CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 make verify V=1`
  passed before final timing: 171 result groups, 2423 passed and 22 ignored test
  executions across profiles/examples, zero failures; strict Rustdoc passed.
- Final three alternating hybrid pairs: medians 111.885868 → 61.102038 s,
  45.39% lower latency, 1.831× speedup. All six outputs are byte-identical and
  satisfy independent literal/cardinality/current-state oracles. Five separate
  narrow FTS pairs: 0.682530 → 0.674626 s; no material improvement established.
- New genuine Samply recording after ordinary timing: parsing 1.19% and layout
  scanning 0.39% inclusive observed CPU, versus 53.64% and 31.69% in the earlier
  baseline. Scopes overlap and host conditions differ; these are profile shares.
- Measurement capture only in the isolated clone: `2db40fd82fd8b504ebb2040b96b248031ce5d791`.
  Final copied binary SHA-256 `bd2c79b6b3dff6858fd021ca7af521f9ceff735279844d76638f1879fa9fb0b6`.
  All 106 root product inputs match this capture; root remains uncommitted.
- Independent code and data reviews completed without material findings. Final
  canonical 4334 corpus file hashes match; native read-only oracle confirms
  100000 vectors/keys, dimension 768, no missing/orphan/error/reserved state.
- Full results, commands and evidence:
  [search report](../../zkit/letopis/checks/issue-65/search-optimization/report.md).
- Final consistency audit passed: all 284 historical evidence artifacts unchanged,
  140 local Markdown links resolve, unrelated existing diff is byte-preserved,
  product/test/corpus identities still match after documentation edits.
