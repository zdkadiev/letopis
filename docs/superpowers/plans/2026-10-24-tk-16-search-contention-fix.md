# TK-16 — Concurrent search/drain defect correction

Status: fix implemented; full verification running; supplement to the completed local RAM assessment.
User confirmed search must work during drain and asked to finish TK-16. This
restores the existing MC-6 / D-62 behavior; it does not introduce a new runtime,
provider, lock file, schema, service or CLI command.

## Contract and investigation

Sources: [mechanics](../../zkit/letopis/mechanics.md#mc-6--чтение-поиск-и-обход),
[architecture](../../zkit/letopis/architecture.md#program-structure),
[RAM report](../../zkit/letopis/checks/TK-16/report.md).

Search reads configuration and publishes query diagnostics through independent
SQLite connections. Drain writes short transactions in vectors.db. The generic
connection currently has zero busy timeout, so an ordinary competing writer can
make reset_vectors or query-error publication fail with DatabaseBusy. Three regression tests reproduced internal_error at these boundaries before the
fix and pass afterward; the original capture did not retain its private error cause.

## Steps

- [x] Reproduce contention before configuration and after inference, for successful
  query embeddings and provider failure with successful FTS fallback. Keep old
  diagnostics and exact search results visible in assertions.
- [x] Apply the smallest verified SQLite writer synchronization fix. Preserve
  synchronous I/O, pinned read snapshots, identity rechecks, rollback and safe errors;
  waits must be bounded and cache/read policy must not change globally.
- [ ] Run targeted tests, make verify and relevant host/Linux storage validation;
  run mutation checks over changed production code. Record unrun tools/platforms.
- [ ] Review architecture and changed files; update report, task card, GitHub and
  Engram with regression evidence. Preserve original failed captures and binary
  hashes. Old RAM numbers remain historical to the original build, not new claims.

4 GB hardware / full RAM accounting remains not assessed after a software fix.
No automatic merge/push is authorized by this correction.

## Implemented policy and evidence

`SqliteStorage` configures only its independent vectors writer with a5-second
SQLite busy timeout in both open paths. The default/cache/read timeout stays0.
There is no new application retry loop, public trait, schema, HTTP call or async
runtime. BEGIN IMMEDIATE still precedes identity reads, commits and rollbacks are
unchanged. This tolerates ordinary short vector-write contention; sustained
contention beyond the bound remains an error, not an indefinite wait.

Regression command: `cargo test --locked --offline --test search_contention`: old
code0passed/3failed; fixed/restored3passed. Removing either open-path wait or
setting the wait to0 is caught:3caught/0missed/0timeout/0unviable selected manual
semantic controls. Native policy test checks both open paths and zero cache/read
timeouts. See `.artifacts/tk16/contention-*.log` and semantic-controls receipt.

Fresh independent review found no actionable issues, conditional on pending gates.
Its limits are retained in `.artifacts/tk16/contention-code-review-01.md`.
Ruling: use bounded SQLite writer waiting rather than new application-level locks
or retries — existing short vector transactions already serialize this resource —
cost if wrong: contention beyond5seconds still fails and needs separate diagnosis.
Ruling: keep original RAM evidence pinned to the pre-fix build — it must not imply
the fixed build was remeasured — cost if wrong: misleading hardware/budget verdict.

The first host attempts exposed only Rustdoc backticks and test-helper borrowing
Clippy findings; fixed without lint suppression. First Linux capture fails the
same helper lint. Fresh host03/Linux02 gates verify the final code. Earlier raw
failures remain retained. Mutation run01 interrupted to avoid copying ignored
corpora; run02 rejected incompatible copy flags before testing. Run03 uses
ignore-aware copies, default timeout policy and the complete root crate.
