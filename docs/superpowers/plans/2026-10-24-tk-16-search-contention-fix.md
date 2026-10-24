# TK-16 — Concurrent search/drain defect correction

Status: software correction and verification complete; supplement to the completed local RAM assessment.
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
- [x] Run targeted tests, make verify and relevant host/Linux storage validation;
  run mutation checks over changed production code. Record unrun tools/platforms.
- [x] Review architecture and changed files; update report, task card, GitHub and
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

Fresh independent review found no actionable issues; the required gates subsequently passed.
Its limits are retained in `.artifacts/tk16/contention-code-review-01.md`.
Ruling: use bounded SQLite writer waiting rather than new application-level locks
or retries — existing short vector transactions already serialize this resource —
cost if wrong: contention beyond5seconds still fails and needs separate diagnosis.
Ruling: keep original RAM evidence pinned to the pre-fix build — it must not imply
the fixed build was remeasured — cost if wrong: misleading hardware/budget verdict.

## Final verification and retained failures

Final native runs are `contention-final-host-01` and
`contention-final-linux-01/container`, both PASS. Each runs make verify:
**487 passed / 0 failed / 5 ignored per debug and release profile**, strict
Rustdoc, schema parity/runner checks, native capabilities, copied executable and
linkage; the real60-second provider deadline passes separately. Input hashes match
the final tree (271 host inputs,155 Linux inputs). No source changes followed.

Earlier lint failures remain captured and were fixed without suppression. One
verification attempt overlapped heavy mutation builds and failed three provider
readiness watchdogs; the same gates passed when heavy jobs were serialized with
RUST_TEST_THREADS=2. Timeouts were not increased. Interrupted mutation attempts
remain raw evidence and are not represented as complete independent sweeps.

Release CLI smoke on an owned copy of the100k corpus:6/6 searches returned5results
while the owned drain was alive. Current vectors2458→2553, final100000 knowledge,
zero failed-with-error/reservation/query-error rows. Its wrapper failed a final
count assertion: it counted all failure-table reservation rows; the exact earlier
count was not retained. Independent response and coherent-final-state verification
passes in `contention-smoke-01/functional-verification.json`; this does not turn the
failed wrapper into a pass. This is functional evidence, not a new RAM measurement.

## Mutation verification and equivalent cases

Whole-source classification is complete incrementally, not one uninterrupted run.
The old inventory2264 produced509caught/5missed/0timeout/1737unviable/13equivalent.
The18proved equivalents below include an initial excluded SHA operator and four
reviewed old misses. The remaining real survivor was the exact initialization
retry deadline. A private pure predicate and deterministic0/4/5/6-second test
now catch that boundary; the public behavior/clock/runtime are unchanged. A safe
invalid-path diagnostic regression also strengthens previously uncovered errors.

The final2266-case inventory reuses2254 unchanged, source-mapped cases and freshly
runs all12 changed/new predicate cases: **4caught/0missed/0timeout/8unviable**.
Final classification: **510caught / 0missed / 0timeout / 1739unviable**, plus
17reviewed equivalents in the inventory and1equivalent excluded before inventory.
Unviable means the mutated code fails compilation (including denied unused
variables), not that product compilation failed; it is not counted as a caught test.
`contention-final-mutation-01.json` pins exact operators, source hashes and mappings.
Selected native/module tests are followed by full-suite fallback for survivors.
Twenty-six apparent catches containing readiness-watchdog failures were discarded
and replayed quietly; no such catch is accepted as regression proof. The final
12-case run selects21 connection/schema tests and its four kills all fail the
independent exact-deadline assertions. Test assertions and timeout388s are unchanged.

Equivalent exclusions in `.cargo/mutants.toml` are anchored to exact operators:

- SHA hex decoding: `(hi<<4)&lo==0` for all256 nibble pairs, hence OR==XOR.
- Eleven open/permission flag expressions: operands have disjoint bits in the
  macOS/Linux ARM64 ABI and locked bundled SQLite header; OR==XOR.
- Two Git command predicates: every existing writing command stays hardened;
  only read-only hash-object gains fsync config, returns the same SHA1/SHA256 and
  creates no objects. Private argv spelling is not a contract. Recheck new commands.
- Cache attachment URI: both encodings decode to the same canonical absolute Unix
  filename, including Unicode/reserved bytes; mode=ro and native read-only behavior
  are identical. Recheck if supported path semantics change.
- Two version comparisons: locked static SQLite3.53.2 returns3053002, so `<`, `==`
  and `<=`3051003 are all false in this build. Recheck Cargo.lock/native-stack changes.
- Config root validation: duplicate keys are rejected and all four allowed keys
  remain mandatory without defaults, so weakening the first cardinality/unknown-key
  conjunction admits no new valid shape. Recheck when the schema changes.

Independent exhaustive/native checks are retained in
`contention-{equivalent,flags-equivalence,git-equivalence,uri-equivalence,version-equivalence,config-shape-equivalence}-*.json`.
No file or function is excluded wholesale; the real deadline survivor is tested,
not excluded. Architecture review confirms only storage policy/private helpers and
behavioral tests changed; public traits, module import direction and lib.rs wiring
remain unchanged. Original4GB/full-memory criteria and G-1 remain not assessed.
