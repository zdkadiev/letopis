# Remaining performance work

These are local issue drafts, not published issues or approved algorithm changes.
The [report](report.md) and receipts define their measured scope.

## Reduce repeated immutable source-blob work

At 10k, ordinary check has a 2.0079-second baseline median (three observations).
An instrumented check performs 10,000 historical reads and 10,000 confined live
captures; historical addressed reads occupy 1.780 inclusive wall seconds in that
diagnostic execution. Sources occupy 100 sharded files with distinct line selectors.
Owners: `ProvenanceService`, `HistoricalObservations`, and Git addressed reads.

Hypothesis: bounded reuse of already verified immutable tree/blob data could avoid
repeated tree traversal, decoding and hashing for different ranges of one source.
Do not cache live captures across bindings: existing tests require detecting edits
between observations. Do not add a cross-command cache or weaken corruption checks.
First establish a controlled same-file/range-count experiment, then compare at
least ten valid ordinary samples per side. A provisional goal is 25% lower check
median with equal flags/findings and a documented bounded memory allowance; this
is a proposed improvement criterion, not an NFR or promised result.

Regression focus: SHA variants, missing/corrupt objects, selector boundaries,
multiple author bases, cache capacity/exhaustion, failed reads, live edits between
observations, and index-lock reader overlap. Real providers remain outside scope.

## Attribute and improve 100k first publication

The current baseline timed out at 120.012554 seconds, consuming 105.361411 CPU
seconds with approximately 346.52 MiB wait4 peak RSS. No initialized 100k query
measurements exist. The responsible split between publication/storage/SQL and
source observation is unproven; the 10k bulk-refresh dump does not establish it.

Start with bounded operation timings around the publication and storage owners in
a separate diagnostic build; preserve the timeout receipt and raw input hashes.
Change one suspected workload variable at a time. A first measurable criterion is
correct completion within the existing 120-second window on the same corpus and
host settings, followed by three successful independently initialized attempts.
Do not claim an initialized-query improvement before initialization succeeds.

Regression focus: all 100k knowledge/source nodes and 500k edges, source hashes,
last-valid invalid-file policy, atomic publication/revision, cancellation/rollback,
memory growth and a reader overlapping the writer. Any schema/index or algorithm
change needs its own agreed design and relevant native/storage checks.
