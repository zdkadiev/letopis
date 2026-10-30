# Issue-67 evidence index

All raw evidence is local and ignored under `.artifacts/issue-67/`. These paths
refer to captured artifacts, not mutable Cargo executables. Keep historical
directories intact; use a new path for every future campaign.

| Evidence | Repository-relative directory | Important records |
| --- | --- | --- |
| Frozen pre-optimization release | `.artifacts/issue-67/before-20261030-task1/` | `build.json`, `release/build.json`, `release/cargo-build.log`, `source/`, `release/product/`, `base-equivalence.json` |
| Initialized 60-element check baseline | `.artifacts/issue-67/baseline-check60-20261030-task1/` | `manifest.json`, `fixture/corpus.json`, `corpus.json`, `samples.jsonl`, `summary.json`, `coverage.json`, `completion.json`, archived `harness/` and cases |
| Task 1 harness validation | `.artifacts/issue-67/task1-validation-20261030/` | RED/GREEN, mutation and full-suite logs, `index.json` |
| Optimized frozen release | pending | Not yet captured |
| Paired timing / initialization / contention / profiles | pending | Not yet executed |

Frozen build identity:

- Actual source revision: `881cc6417073110e852fbebf2b9bce5ed1ea3d29`.
- Base-equivalent product revision: `32ca0af282a1321d0137b4aba115c6fddc086259`.
- Product manifest: 156 files; SHA-256 of canonical encoded manifest
  `26a6783e7f7bc598d3f875a9eda3d5cfb0312125a17a574dc05bc6b88cd2c8df`.
- Executable SHA-256:
  `a132acb0a19c24440e8e97b881657c7b7adcc5c17052469f5dbc9d1e1b336f62`.
- Original release-receipt SHA-256:
  `49cc06cedb55af346802345af80fb2a8deceb020238fe8581b3524fec44f5e28`.

The baseline used the original single-revision survey runner on the frozen
executable while product sources still matched the base. Its archived harness
records the exact version executed; subsequent comparison-harness fixes do not
rewrite that archive. Use the outer frozen `build.json` for future before/after
comparisons after product source changes.

The comparison tests exercise real generated fixtures, files and owned fixture
processes, with only expensive release compilation replaced by a test builder.
They do not establish real optimized-binary correctness or profiler availability.
See [report.md](report.md) for measured values and remaining work.
