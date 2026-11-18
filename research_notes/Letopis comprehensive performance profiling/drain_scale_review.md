# Drain-only adjunct review

**Verdict: PASS for bounded runtime validation**, after exclusive measurement ownership transfers. Static review only; no execution, test, product, build, observer invocation, or active catalog progress read. Runtime results and timing remain unknown.

## Findings

No intrinsic blocker found for the requested three drain cases.

- [Selection](evidence/drain-scale-20261117-prep-v1/drain-cases.json) contains only `embed --drain` for synthetic sparse 2, 100, and 1000, mock provider, one validation and three measured repeats per case. The separate six-case search/drain proposal remains historical; search is postponed and not claimed executed.
- [Launcher](evidence/drain-scale-20261117-prep-v1/launcher.py#L134-L155) calls the unchanged reviewed driver through its CLI, preserves that new campaign's generated original catalog and manifest, then freezes the three-case catalog and matching manifest **before** any validation. It does not write the original catalog or driver. `verify_campaign` and the driver's own manifest, inventory, source, build and validation-identity gates remain in effect.
- Fresh read-only SHA-256 checks matched selection, proposal, producer, launcher and driver to their recorded pins. All **116** stock receipt input hashes and **130** observer receipt input hashes matched current files; both archived executables matched their receipts. This is a static hash check, not a rerun of the driver's complete source/inventory and UUID gates; those run again during preparation/execution.
- Each attempted drain requires queue N→0, N distinct search hashes, zero failures and **N provider items**, not N HTTP requests. Native observer must report N current-configuration covered/knowledge nodes, 32 dimensions, populated verdict and explicitly zero missing, eligible and invalid counts. [Adjunct checks](evidence/drain-scale-20261117-prep-v1/launcher.py#L65-L75); [driver checks](tools/profile.py#L789-L794).
- New evidence path and owned scratch are separate. Product environment is allowlisted, provider endpoint is numeric loopback with a fixture-only dummy key, and detached work is suppressed by the owned lock during setup. Fixture and source hashes are rechecked; no real provider or product execution in the author repository is selected.
- Driver records planned rows, timeouts, skipped or setup-failed slots and accounting; no automatic execution retry. Adjunct rejects failed or missing rows and stops further stages; absence of `adjunct-execution-complete.json` means incomplete even when an individual driver row says valid. Preserve all raw partial results.

## Runtime qualifications

- The 120-second cap applies to individual product commands. The driver's 1,800-second budget is **per CLI stage invocation**, not one aggregate cap across all six validation/measurement invocations; preparation has no outer launcher timeout. Existing disk and post-observation RSS guards are soft, not instantaneous limits. No broader runtime guarantee is inferred.
- The adjunct checks all three measured rows after each driver measurement invocation. The driver's own invalid-row handling stops further repeats, but an adjunct-only mismatch may be detected after the other repeats in that invocation. It stops subsequent stages and is not a successful adjunct run.
- Existing [four pure-test results](evidence/drain-scale-20261117-prep-v1/launcher-tests.txt) and mutation evidence were read, not rerun. Fixture uniqueness, queue counts, provider items and native coverage require actual runtime receipts before any performance claim.
