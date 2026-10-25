# TK-19: production check and lifecycle acceptance

Date: 2026-10-25. Implementation and macOS host acceptance PASS.
Whole-branch review: **Approved within accepted scope; M1 ADDRESSED; no open findings**.
No Critical or Important findings were reported. The combined
check/status task stays `in_progress`: status belongs to TK-28, traversal to TK-23.

## Source and reproducibility

Branch/worktree: `tk-19`, `/Users/zdkadiev/.codex/worktrees/tk-19/letopis`.
Original base: `c080ddb20418327a65b6d49768879920acb03129`.
Task 1: `f1c6bfd656377bbe17d05034e4b031000c197c02` (independently Approved).
Task 2: `41d1b9a66739a3074f114ad2d7e084b1439b0afc` (independently Approved).
Final commands ran on that Task 2 HEAD/tree `b76495b71c97a88969d6118b62cbdfa388edf682`
plus the uncommitted Task 3 acceptance/runner files. The containing test/docs commit
preserves these exact tested inputs; no product source changed in Task 3.

[verification.json](verification.json) preserves the complete SHA-256 inventory
of 269 tested source/test/build inputs, exact commands/exits, platform,
mutation replacements/restoration hashes and bounded transport command records.
Canonical inventory SHA-256: `07e46b1e9392226f3e715bddf5649dd8aa3e4a4749507c297ef2602ee23aa4f1`
(sorted path-to-hash JSON, compact separators). Documentation is excluded from
this tested-source digest to avoid a self-referencing receipt. New private logs
and reusable drivers are under `.artifacts/tk19/task3/`; historical TK-18/TK-17
reports/manifests are unchanged.

Host: macOS 26.6.2, Darwin 25.6.0, arm64/aarch64-apple-darwin.
Toolchain: Rust 1.95.0 (`59807616e`), Cargo 1.95.0 (`f2d3ce0bd`). All Cargo tests
used `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target RUST_TEST_THREADS=2`
and `--locked --offline`. Python runner tests/smoke used the existing read-only
`/Users/zdkadiev/.codex/worktrees/tk-11/letopis/.artifacts/tk11/python-env/bin/python`
(Python 3.12.0, jsonschema 4.26.0), with no installation or external request.

## Delivered behavior and owning acceptance

| Responsibility | Evidence |
| --- | --- |
| Read-only exact Git tree probe; missing/foreign/wrong-type classification, no peeling/ref/object writes, policy/integrity errors | `tests/snapshot_verification.rs`: 4 tests, both SHA-1/SHA-256 |
| Refresh before one read; caller-owned pinned state; OID dedup; refresh/read/Git failures without partial success; exact owners before probes | `src/service/check/tests.rs`: 5 tests |
| Actual product JSON/exit, current parser errors/dangling/includes cycle, staged repairs, whole corpus/paths, normalized duplicate owners, arbitrary relation names and metadata, snapshot checks | `tests/check_command.rs`: 7 tests |
| Tracked target rename/delete/restore and directory move; literal dangling ID, old node absent, created_at=42 preserved, unchanged source bytes after each check | `check_tracks_file_and_directory_rename_without_touching_sources` |
| Anchor old→new, removal, restore and authored source repair; old ID disappears and created_at=42 remains | `check_tracks_anchor_rename_removal_and_repair` |
| Invalid edit retains the full last-valid metadata and reports the current parser error; repair changes hash/FTS without changing authored time | `check_keeps_last_valid_projection_while_reporting_current_file_error` |
| Separate services edit distinct files; Barrier releases both only after edits; one final pinned view finds both literal IDs/FTS texts and authored times 42/73 | `concurrent_check_publishes_edits_from_both_writers` |
| Repeated unchanged real binary check keeps exact findings, marked/unmarked bytes and cache revision | `repeated_check_preserves_corpus_and_revision` |
| Current transport runner expects successful `check {}` with empty errors, permits cache initialization, and rejects malformed input before I/O | `tests/cli/test_runner.py` and bounded actual-product collector |

Lifecycle tests extend completed product behavior and started GREEN; no initial
behavioral RED is claimed. They use actual `CARGO_BIN_EXE_letopis`, except the
explicit deterministic service concurrency case. There are no sleeps, global cwd
or environment changes, production hooks, configured providers or external model
calls in these new tests. Reports from concurrent checks may see different
committed revisions; the test asserts the final state and both successful calls.

## Executed verification

| Command (from repository root) | Result / private log |
| --- | --- |
| `cargo test --locked --offline --test snapshot_verification --test check_command --test check_lifecycle` after all product controls were restored | PASS 4+7+5, `restored-targets.log` |
| Prepared Python interpreter above, `-B tests/cli/test_runner.py` | PASS 20/20, `python-runner-restored.log` |
| Same interpreter, `-B /private/tmp/tk19-transport-smoke.py` (preserved as `.artifacts/tk19/task3/capture_transport.py`) | PASS 7 actual-product cases, 28 bounded commands; `transport-smoke.log` and `transport-smoke/manifest.json` |
| `make verify V=1` with the Cargo environment above, once on the final restored source | PASS exit 0: fmt-check, all-target check, Clippy `-D warnings`, debug tests, strict Rustdoc and release tests; `make-verify.log` |
| `git diff --check`; affected relative Markdown links checked | PASS |

Integrated root counts: debug **533 passed / 0 failed / 5 ignored**;
release **533 passed / 0 failed / 5 ignored**.
These are root Cargo result groups; nested child-driver observations are not
added again. Three ignored functions are subprocess entrypoints driven by parent
tests. The other ignores are the historical real 60-second provider deadline and
existing-corpus observer; neither was explicitly run. There are **21 new Rust
tests** across Tasks 1–3 (4 snapshot + 5 service + 7 CLI + 5 lifecycle).

The requested direct `python3 -B tests/cli/test_runner.py` first ran with system
Python 3.9.6/jsonschema 4.23.0: 19 passed, one existing Python 3.11+ preflight test
errored. This was an environment mismatch, retained in `python-runner.log`;
prepared-environment runs pass all 20. Adding the standard `__main__` entrypoint
makes direct invocation actually execute the suite. An initial lifecycle shell
wrapper also exited 1 after all five tests passed because zsh reserves `status`;
the later fully captured restored-target and integrated runs exited 0.

## Selected mutation controls

Each control compiled and failed its named behavioral observer; every changed
product/runner file was restored byte-for-byte. Exact replacements, commands,
logs and matching SHA-256 pairs are in [verification.json](verification.json).

| Control | Observer / result |
| --- | --- |
| Suppress removal publication | File/directory rename test: dangling finding missing, exit 101 |
| Replace authored projection timestamp with 43 | Same test: literal expected 42 differs, exit 101 |
| Drop changed `b.md` from detection when a previous row exists | Concurrent test: final BirchEdited FTS match missing, exit 101 |
| Restore historical unavailable-check runner expectation | Focused Python transport test: expected successful exit differs, exit 1 |

Restored product hashes: `src/service/document_refresher.rs` =
`3a0eb148f4b51b0f7400cccfce9a6258b4525ab7fe888d2cf56a0ada80b8f3be`;
`src/service/projection.rs` =
`a30df7d9b88aedea75e332f16674f15f99c3fd2d746e3432c7740127c1f4501b`.
Across Tasks 1–3: 1+4+3 product controls and one runner control, **9 detected**.
Task 1 distinguishes its initial compiler RED from later behavioral RED; Task 2
retains its own binary RED/service compiler RED and precise restored runs. Their
historical task evidence remains separately attributed.

## Structural self-review and limits

Reviewed actual files and constructor calls: application-owned `Checker` and
`SnapshotVerifier` contracts/reexports in `service/`; generic concrete CheckService
in `service/check.rs`; GitCli implementation in `git/cli.rs`; existing pure domain
validation and shared GraphDiagnostics; thin `cli/handlers.rs`; real refresher,
confined reader, diagnostics, Git and background assembly/dispatch in `lib.rs`.
Service imports domain/application traits, never concrete adapters. Storage/Git
implementations remain separate, synchronous and RAII-owned; errors release their
read views/locks. Check opens one pinned read after refresh and does no inference
in that view or write transaction. No body storage, schema, Markdown grammar,
corpus writer, async runtime, dependency, process or durable cache was added.

Task 3 changes only acceptance tests, runner expectation/entrypoint and scoped
documentation. Test functions/helpers remain below 60 lines and independently
assert literal IDs/times/messages. Public product diagnostics use fixed safe
messages; malformed input still fails before initialization. The new tests and
bounded transport run used no configured provider; existing full root tests use
their controlled synthetic loopback fixtures.

No native/build/vendored/dependency input changed; storage/native and Linux
runners were not rerun. The full historical CLI campaign and separate historical
proof manifests were not rerun or overwritten. No Linux/x86/Windows/NFS, physical
power-loss, handled-signal, external-provider/model-quality, capacity/pilot,
G-1/G-2 or v1 claim follows from these host checks. Full source bytes/ranges and
provenance remain TK-21, status TK-28 and traversal TK-23. Handled SIGINT and a
single cancelled JSON/exit-1 guarantee remain TK-29. The current runtime installs
no SIGINT handler and uses the default process signal behavior: SIGINT terminates
the foreground process, without a cancelled-JSON or exit-1 guarantee. Cancellation
handling was not tested in this acceptance. Whole-branch review found no
implementation findings; M1 clarified this scope; scoped re-review confirmed it ADDRESSED with no new
findings. Engram recording completed at revision 259.
No merge, push, publication or GitHub update was performed.

## Whole-branch review and M1 documentation correction

2026-10-25: independent whole-branch review of
`c080ddb20418327a65b6d49768879920acb03129..04742af23ec7660d18de872f05831955209b9bbd`
reported no Critical/Important or implementation findings. Minor M1 identified an
unqualified cancellation promise in the spec and plan summaries. This bounded
documentation correction distinguishes delivered invalid-input/infrastructure
responses from deferred handled SIGINT. The existing default signal behavior
does not guarantee a cancelled JSON or exit 1; TK-29 owns that work.

M1 clarified; scoped re-review Approved, M1 ADDRESSED. The 269 verified product/test/build input
hashes and their canonical digest remain unchanged. Validation covers affected
relative links, receipt JSON and `git diff --check`. Product suites were not rerun:
this correction changes no product or test input, and the final reviewer explicitly
requires only bounded review of the documentation. Historical execution evidence
and the prior integrated verification remain unchanged. Engram recording completed at revision 259; main/GitHub integration is not performed.

Documentation validation: `python3 -B .artifacts/tk19/task3/m1-docs/validate_docs.py`
PASS (16 relative link targets, all 269 hashes equal to the accepted checkpoint,
unchanged canonical digest); `python3 -B -m json.tool
docs/zkit/letopis/checks/TK-19/verification.json` PASS; `git diff --check` PASS.
New private validation output: `.artifacts/tk19/task3/m1-docs/validation.log` and
`validation.json`. These checks do not rerun or replace the historical product suites.


## Final administrative closure

2026-10-25: all three task reviews Approved. Final whole-branch review found no
Critical/Important or implementation issue; its sole Minor M1 was clarified in
cc95d1250433f6e2281c6481d5d3bf430ce360ff, and independent scoped re-review marked
M1 ADDRESSED with no new findings. Private review records remain in the retained
SDD workspace. Shared Engram revision 259 records the implementation and evidence.

Executor decisions: retain private verification/SDD evidence (local disk cost);
update the current CLI runner expectation for working check (small extra runner
maintenance, historical evidence unchanged); retain handled SIGINT ownership in
TK-29 (current CLI has no handled-cancellation JSON/exit-1 guarantee).

Accepted TK-19 check implementation is complete. Combined check/status task
criteria remain pending status TK-28; traversal TK-23, source provenance TK-21,
handled cancellation TK-29 and platform/pilot gates are not accepted by this work.
