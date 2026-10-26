# TK-27 final whole-branch review

Date: 2026-10-26. Reviewed range: `6f59bbb..02b97365820652c851caca6371d2990ebd9c0573`. Product source: `14d1e4773974030430936e2aff3f76138dd5e1ac`; the subsequent commit changes documentation and the executable design model only.

## Scope and method

Read the approved specification and implementation plan, the supplied whole-branch diff package, the SDD ledger/rulings, the final TK-27 report and verification receipt, the independent controller audit, and current implementation/test context. Followed the broad code-review contract: considered ordinary consumer expectations as well as enumerated acceptance criteria. Reviewed production changes, mechanical interface migrations, new integration tests, the model and changed canonical contracts. No product files, Git state, or passing verification gates were changed or rerun. This report is the sole reviewer-created file.

## Strengths

- The correction stays at the existing application/storage boundary. `src/service/embedding.rs:111` obtains a short database observation before parsing/validating the config and releases that read before reset. Storage owns the conditional write; it never reads the configuration file. The application owns orchestration through `Storage`, `EmbeddingFiles`, `MarkdownCodec`, and `EmbeddingProvider`. Domain code, composition in `src/lib.rs`, dependencies and database schema remain unchanged.
- `src/storage/vectors.rs:28` checks both predicates inside one `BEGIN IMMEDIATE` transaction. Checking desired identity first preserves newly stored vectors and attempts for duplicate observations; comparing the current identity against the original observation prevents P(A)/Q(B)/late-P destruction. The absence case follows the same rule. Early returns roll back an unchanged transaction; reset writes and native index reconstruction commit together. The shared typed identity reader rejects malformed metadata instead of treating it as permission to reset.
- `ConfigState::Changed` is propagated deliberately. Drain returns `ConfigChanged`; direct query and readiness stop before provider calls. Search accepts only `Ready` in `capture_vector_config`, then verifies the pinned identity/dimension; `Missing`, `Invalid` and `Changed` retain FTS. Traversal continues through its single scoped graph view. No reset/re-read loop or new CLI response state was added.
- Existing publication guards remain intact: reservation commits before HTTP, HTTP runs outside the write unit, and late vectors/errors/query diagnostics can write only through a transaction matching the captured identity. Text-addressed storage continues to prevent an old result from representing changed text.
- Tests have useful independent oracles. The process regression pauses a real service process after validation and checks B identity, literal vector bytes, attempts, cache state and zero old-provider calls across four actions. Storage tests cover absent initialization, conflicting observations, duplicate desired identity, malformed identity, and a pinned reader surviving reset. Existing rollback tests now pass the explicit observation and still force failure after native schema changes.
- The new CLI tests exercise actual drain processes: SIGINT is delivered only to an owned unreaped child; the committed reservation survives, the lock is released, and two later processes cannot renew the budget. Cache-loss/reindex verifies both the exact reused vector and the single missing-text HTTP request. Long-input rejection verifies the full request, `truncate:false`, safe durable diagnostics, exhausted retry state and retrieval of a unique final FTS token.
- MC-3 is the canonical conditional-reset description, with X-2/I-8 linking to it. The model adds the accepted stale-observer schedule and a separately selectable negative control. The report distinguishes synthetic provider evidence, ignored child entrypoints, executed parent scenarios and unperformed project gates.

## Issues

### Critical — must fix

None found.

### Important — should fix

None found.

### Minor — nice to have

None found.

No significant implementation deviation from the approved plan was found. The example signature migration is justified by its existing use in all-target and standalone native validation. The shared storage identity parser removes duplication without crossing layer boundaries.

## Evidence assessed

The controller audit records 785 passed, 0 failed, 5 ignored in each of debug/release on macOS arm64 and Linux arm64, across 61 root executables per profile. Eight nested helper successes are correctly excluded from each root total. The actual 60-second deadline scenario was separately executed on both platforms. The receipts also record formatting, all-target checking, Clippy, strict documentation, schema parity, runner tests, native capability, standalone execution and linkage checks.

The five ignored root entries are accounted for: three child entrypoints exercised by their parent scenarios, the separately executed real deadline case, and the external-corpus case which was not run. The first Linux sandbox/buildx failure happened before product execution and remains distinct from the successful authorized retry.

The independent controller audit checked current input hashes and the common host/Linux inputs. During this review, a read-only SHA-256 check independently confirmed all 10 required current source files, all four canonical document/model files, and both referenced manifest hashes against `verification.json`; no mismatch was found. `git rev-parse HEAD` confirmed the reviewed head, and the final delta after product revision `14d1e47` contains only the nine reported documentation/model files.

Reviewed mutation receipts support the behavioral tests: removing either reset guard loses stored state; increasing the attempt budget is detected by the persisted count; dropping the final FTS token is detected by retrieval; discarding matching vectors is detected by the unexpected recomputation. The restored-source receipts are distinguished from mutant failures. This review relies on those recorded successful gates rather than claiming a fresh execution.

## Recommendations

Resolve the deliberately pending whole-branch-review labels in the report, task card, specification, plan and machine receipt using this verdict, preserving the distinction between reviewed head and tested product source. Record the executor's rulings on the explicit scope exclusions below. No product correction is requested.

## Declined to judge

- Atomic agreement between arbitrary external config-file edits and SQLite: explicitly excluded by the accepted conditional-observation contract; the implementation has no shared file/database transaction or cooperating editor protocol.
- Temporal generation detection across identity cycles or database replacement: the accepted design intentionally compares configuration identity and excludes persisted generations. This review does not claim that identity equality proves no intervening reset occurred.
- A lifetime retry budget across intentional configuration resets or deletion of `vectors.db`: explicitly excluded; reset intentionally clears attempt state, and the proven budget applies while that identity/state is retained.
- Graceful handled cancellation and a `cancelled` JSON response: TK-29 owns that behavior. TK-27 proves actual SIGINT termination, durable reservation and lock release, not a handled response.
- Expanded status diagnostics: TK-28 owns their completion; this review checks the existing safe persisted errors and consumer fallback relevant to TK-27.
- Real provider/model context limits, model quality, throughput, hardware capacity and arbitrary compatible-server non-truncation: local HTTP fixtures establish request/persistence behavior, not those deployment properties; no universal text limit is claimed.
- Physical power-loss durability, full DR-5/DR-8 and system-wide queue/recovery drills: the present process/native evidence does not execute those separately scheduled drills, including TK-37.
- Completion of TK-17/G-1 or G-2 and production deployment readiness: explicitly unestablished project gates; local authorization and this code review do not close them.
- Platforms other than the recorded macOS arm64 and Linux arm64 environments: no unrecorded platform result is inferred.

## Assessment

**Spec compliant:** Yes.

**Code quality:** Approved.

**Ready to merge? Yes, for the approved TK-27 scope.**

The final implementation preserves the intended architecture and closes the demonstrated stale-reset defect with an atomic conditional update, finite conflict propagation and meaningful regression evidence. No actionable finding remains; merge/push still require the user's authorization, and the deferred project gates above remain separate from this code-readiness verdict.
