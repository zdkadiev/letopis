# Exact flat vector-index candidate

Status: final six-file source frozen and independently reviewed; focused tests, candidate make check, six viable hand mutants and exact source restoration complete. No background work remains. No ordinary full-corpus timing or real-provider inference is run by this worker.

Baseline: `2db40fd82fd8b504ebb2040b96b248031ce5d791`, cloned from the prior combined search source. The six final paths and exact hashes are in `source-manifest.json`; `candidate.patch` includes the new private module explicitly. Shared/root source, old evidence, existing fixture data and other agents' targets were not edited. An attempted root-target APFS clone was stopped; all accepted builds/tests use the fresh owned `target-build/` directory with two Cargo jobs. The incomplete copied `target/` is unused.

## Physical transition

Fresh `vectors.sql` and configuration reset now create exact `flat/cos` using the same pinned vec1. Schema version remains 1. Search candidate K, eligibility, complete channel ranking, filters, metric/ID ordering, freshness observations, embedding identity and provider behavior are unchanged.

`bootstrap_vectors` performs existing structural schema initialization/validation, then private storage-owned `vector_index_prepare`. That operation lives in the cohesive `src/storage/vector_index.rs`; general schema logic stays in schema.rs and the main concrete VectorStorage remains its trait owner. There are no public migration APIs, config switches, wrapper structs, new dependencies or vendor edits.

The preparer compares the persisted opaque native model with flat/cos and none/cos factory outputs created by the same pinned native module in memory. It does not parse a private model header. This deliberately recognizes only the application's exact simple models; unknown metric/header/version/flags return a safe typed error. Production does not rely on lossy vec1cat JSON: native vec1cat labels any non-L2 distance as cos, allowing corrupt distance 3 to look compatible. The regression test explicitly mutates that pinned native field and proves rejection without repair.

Initial model inspection uses the existing bounded contention retry and a short deferred read transaction. Already-flat returns without requesting a writer. Query-only/read-only recognized legacy none remains unchanged, preserving the previous observational bootstrap contract; a writable opening is required to upgrade it. Structural observational status validation does not invoke preparation.

Writable recognized none acquires an immediate transaction using the existing bounded retry, then rechecks the model after acquiring the writer. A competing opener that already published flat is not rebuilt. Before native conversion the legacy MODEL version field 0 must exist as a positive i64 below i64::MAX, because the native command increments it to reload its model; missing, noninteger, nonpositive or overflowing versions are rejected rather than silently recreated or converted to REAL.

The legacy scan reads original virtual-table vectors one row at a time. Before allocating/decoding, each value must be a BLOB of exactly configured dimensions*4, with dimensions validated by existing native-config checks. The existing checked native-endian f32 decoder then rejects nonfinite values. Additional owned decode storage is bounded to one supported vector (at most 4096 f32 values/16KiB), plus the native row buffer. This is a temporary migration scan, not a persistent payload cache or a new process RSS guarantee.

All scan statements/cursors and factory connections drop before native rebuild. Rebuild and post-model confirmation occur within one transaction; failure rolls back model/config/payload changes. Original rows, rowids, f32 bytes including signed zero, vector keys, embedding config identity, native dimensions, failure attempts and query error are preserved. The native model-version/physical packed-index representation changes intentionally. A separate pinned reader retains its original valid none view, and its next read observes flat.

## Test evidence

The real pinned native library is used in all new tests. Existing historical uppercase SQL fixtures remain untouched; the current canonical DDL mirror is synchronized.

Initial corrected RED (`red-confirmed.log`): three failures demonstrate missing migration/current-state preservation, missing pinned-reader transition, and corruption being accepted. One query-only legacy test already passed. An earlier first RED attempt contained an invalid test assumption about a native config field and is retained separately, not used as the final RED claim.

Second RED (`red-model-guard.log`): eight other scenarios passed, but unknown native distance 3 was silently accepted, proving the lossy-catalog guard bug. Factory model equality makes it pass. Third corrected RED (`red-model-version-confirmed.log`) proves missing MODEL config was silently repaired; the guarded final implementation rejects it. A first compile-only attempt for that case is retained but is not test-failure evidence.

Final focused GREEN (`green-module-final.log`): all 10 pass:

- populated none→flat preserves exact logical/application state and repeated bootstrap makes no further writes;
- an independent pinned reader retains old bytes/mode and refreshes correctly afterwards;
- malformed-short and NaN old payloads are rejected unchanged;
- query-only legacy validation performs no hidden writer;
- injected native index-insertion failure rolls back, same-connection retry succeeds, reopen sees the right model;
- a competing opener preserves the owner's published model-version without a redundant rebuild;
- fresh/query-only flat and empty legacy with retained dimensions remain compatible;
- explicit L2 is rejected unchanged;
- corrupt hidden distance 3 is rejected unchanged;
- missing, nonpositive, noninteger and overflowing model versions are rejected unchanged.

The initial concurrent test used connection total_changes==0, but native setup reported 3 even though the persisted native model-version was not incremented. The final test asserts the actual no-rebuild invariant directly against the owner's published model-version. Removing the writer recheck is separately mutation-tested.

Independent review: `../review/report.md` has no open findings for the final pinned six-file tree after canonical-model, bounded read, config-version and module-boundary corrections.

## Verification ownership and limits

Coordinator explicitly assigned this candidate focused RED/GREEN, hand mutation checks and `make check`; actual-root host native/standalone runner owns full `make verify`, strict docs, release, copied executable, linkage and native parity checks after guarded integration. Those coordinator gates are not claimed by this worker. This change requires that storage runner before retention.

Native-probe agent independently reported full-K parity, exact byte preservation and native-only CPU improvement; this worker has not rerun or independently accepted those measurements. Flat is an exact exhaustive packed-vector scan; no ANN, trained quantizer or top-K shortcut is introduced. End-to-end speedup and matched whole-CLI oracles remain coordinator measurements, and the candidate is not a published/merged feature at this stage.

## Final candidate gate receipts

`CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=../target-build RUST_TEST_THREADS=2 make check V=1` completed with exit 0 on the frozen final six-file tree (`make-check.log`). Root debug suite: 1217 passed, zero failures, 11 ignored; separate fixed-value-lint example suite: 9 passed. Formatting, all-target compilation, denied-warning Clippy and fixed-value analyzer all passed. `make-check-summary.json` records the exact counts. The older partial pre-module-move check log is preserved separately and is not claimed as acceptance.

Six hand-written viable mutations were each caught by a test failure, not a compile error (`mutation-run.log`, `mutations.json`): skip migration; skip writer-owned model recheck; skip finite/size/config prevalidation; accept unknown model; accept invalid version bounds; attempt migration through query-only. An initial script selection matched two identical condition strings and stopped after its first caught mutant; that selector attempt is preserved separately, then corrected to select the intended writer recheck. All six final mutants caught, exit 0.

The original bytes of every frozen source path were restored and rechecked against `source-manifest.json`. Final `vector_index.rs` SHA-256: `caa01aa98ca077dfa6906036bcdadebe1c109c0fbea795243fd354342a8b6a4a`; patch SHA-256: `8e8430da61504f6e84f63aeadb7a6e17f9010c6aa90b4d5e5e59f7b6d624175b`. Reverse patch applicability and whitespace checks pass. `restoration.json` binds the six original hashes, mutation status and successful final-tree check.

No worker build, test, timing or profile process remains. The coordinator owns the actual-root native/storage/standalone runner and subsequent serial whole-CLI timing/profile window. This candidate has no independent release executable pin; the retained actual-root build is intentionally verified and pinned once by the coordinator.
