# Issue 67 evidence index

The authoritative [evidence-index.json](evidence-index.json) distinguishes the
corrected `b488aeb` freeze and fresh campaigns from historical Task 8 `ba4a11d`
measurements. The unchanged baseline remains pinned separately. Every current
after measurement belongs to the corrected executable; before-only historical
publication/censors/contention/diagnostics are explicitly scoped.

`local_artifact_inventory` preserves the original Task 8 point-in-time inventory.
`final_fix.inventory` adds the fresh code/tests/receipts, failed setup attempts,
immutable corrected source/binary, paired samples, profiles/counters/RSS, exact
fixture-membership validation, original independent review and corrected task-8
notes. The old SDD archive is unchanged; a new corrected report is hash-indexed.

Raw evidence is local and ignored under `.artifacts/issue-67/`; binaries and raw
payloads are not committed. Commit receipts produced after each inventory are
explicitly identified as its tail. Issue 65 evidence and protected staged/worktree
state remain unchanged. The original final review found F-I1/F-M1; their fix is
recorded and **scoped independent re-review passed**. `final_acceptance` adds
the exact independent reports, complete execution archive and ruling audit.
Integration/publication has not been performed.

See [report.md](report.md) for current measurements, [compatibility.md](compatibility.md)
for exact platform scope and [review.md](review.md) for findings and rulings.
