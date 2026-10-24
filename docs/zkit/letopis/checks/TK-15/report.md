# TK-15 — Target corpus and production population

> Status: in progress; full product loading and population have not yet run.
> Date: 2026-10-24. Branch: tk-15, base main@c8ede9e.
> Worktree: /Users/zdkadiev/.codex/worktrees/tk-15/letopis.

## Scope

[Specification](../../../../superpowers/specs/2026-10-24-tk-15-target-corpus-design.md)
and [approved implementation plan](../../../../superpowers/plans/2026-10-24-tk-15-target-corpus.md)
define the work. Generator, test-only Rust observer and runner use existing
production CLI/storage interfaces. No product src/dependency/schema/vendor/build
changes as of the current checkpoint. Parallel drain is separate GitHub #49;
this task uses the existing sequential executor. No merge/push/issue closure.

## Evidence captured so far

All raw paths below are worktree-local ignored artifacts, retained independently
of committed reports. Historical receipts remain against their captured source.

| Evidence | Current result | Local path |
|---|---|---|
| Generator and checker | Reviewed after fixing multi-donor packing and code-context links;22focusedtestsPASS | .artifacts/tk15/task1-20261024-01 and task1-20261024-round1 |
| Rust observer | Reviewed;21passed/1ignored entry also exercised by subprocess;fmt/ClippyPASS | .artifacts/tk15/task2-20261024-01 |
| Runner |22generator tests +24runner tests PASS at their unchanged source snapshots; resumed-load fix reviewed | .artifacts/tk15/task3-final-20261024-01 |
| Root gate | make verify V=1 exit0;481passed/0failed/5ignored each debug/release;16nestedfiltered summaries separate | .artifacts/tk15/root-01 |
| Full generated input | Independent checkerPASS;100000knowledge/100000sources/500000edges | .artifacts/tk15/target-01 and full-prep-01 |
| Real model small smoke |60/60current knowledge vectors,768dimensions;historical pre-review runner checkpoint | .artifacts/tk15/task3-real-load-20261024-02 and task3-real-drain-20261024-02 |
| Full configured backlog | pending | not yet captured |
| Full populated state | not assessed | not yet run |

## Full input facts

Seed15; A-2 expected profile; exactly100000marked fragments of2000UTF-8bytes,
sum200000000bytes, mean/min/max2000. Packing3334Markdownfiles, lastfile10elements;
1000source shards, each at most100addressed lines;4334hashed source/Markdown inputs.
Exactly500000distinct edges, including73333includes; independent checker verifies
all targets/ranges and acyclic includes. These are synthetic resource inputs,
excluded from usefulness pilots. A source address is not a separate file.

Generation completed in1.655149seconds; independent validation3.307968seconds.
This is input preparation, not product load or drain performance. Manifest SHA256:
`2825e0e6168d8dedf4b9c0fa770d677533ebd4f55c9f24461f565966d9e5bd96`.

## Verification scope and limits

Root Rust input fingerprint is captured in root-01/inputs.json and matched after
runner-only Python changes. Reuse the gate only while it remains unchanged.
Root log SHA256: `c8656405f481c95c377d794f0cc83b0006e95605aca292d7432e95eafcabcfd3`.
Python/runner final-source results are tracked separately; root checks do not
execute the Python harness or the full100k workload.

Semantic controls are manual, scoped to their captured code snapshots. Initial
generator controls and later fixes are historical separate receipts, not a whole-
crate mutation sweep. No Linux run is claimed for this new harness. No RAM budget,
minimum4GB-machine proof, search latency, real-model quality, memory usefulness,
physical power-loss, full source lifecycle or G-1 success is established here.

## Completion pending

Resumed-load preservation fix1528d8c and scoped re-review are complete. Write
actual command handoff, run full production load under owned lock, then real-model drain and
current-hash/config coverage observation. Populate task criteria and final review
only from those results. Incomplete coverage must remain not assessed.
