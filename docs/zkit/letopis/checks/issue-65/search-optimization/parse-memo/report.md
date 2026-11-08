# Search Markdown parse-reuse experiment

Status: candidate ready for coordinator screening; required verification, ten viable hand mutants, source restoration and standalone release pin completed. No background work remains.

Baseline source: local clone of `.artifacts/issue-65/2026-11-06-native-followup/source`, HEAD `3d820de41ee12385bc734ac4ceeb56b78ba379f9`. The original product worktree and every other experiment directory are untouched. No commit, push, shared corpus mutation, real-provider/shared-corpus embedding call, or 100k timing/profile was performed here. The ordinary root suite uses its existing isolated provider fixtures.

## Change and observation contract

Only `source/src/service/search.rs` changes. `SearchEligibility::get_eligible_links` creates a private `CurrentKnowledge` receiver for that one invocation. Every candidate still obtains source bindings and evaluates source/revision predicates in existing sorted demand order. For each surviving candidate it still reads project revision, freshly calls `FileReader::file_get_by_path`, and handles its errors through the existing mapping. Each available capture is independently SHA-256 hashed. Reuse requires the same normalized path and newly computed exact-content hash. The shared `utils::is_current_knowledge` predicate remains unchanged and runs for every binding, comparing that binding's ID, exact element hash and line range with the parsed data.

Only one most recent successful `ParsedFile` may survive. Invalid/unmarked parsing and missing captures clear it; I/O errors abort the operation and drop it. On a path or content mismatch the old success is dropped before parsing. Successful parses that exceed any budget follow ordinary behavior and are not retained. The cache dies when `get_eligible_links_by_metadata_and_filters` returns or fails, even when the surrounding SearchService remains alive. No current-byte observation, source/provenance result, missing/error classification or link flags are cached.

The retained projection discards its entire edge set and replaces each search string with a zero-capacity String. This keeps the existing eligibility predicate as its single rule owner without retaining Markdown/search payloads. There are no public APIs, trait contracts, composition changes, dependencies, runtimes, SQL/schema changes or source snapshot policies in this candidate.

## Bound

One retained file; source input at most 256 KiB; at most 1024 elements; at most 256 KiB of combined normalized file path and all retained element-ID contents. Element vector `shrink_to_fit` is requested before retention; it is not an allocator hard-cap promise. The remaining per-element values have fixed layout, plus each optional validated snapshot OID (at most 64 ASCII bytes). Retained logical payload is therefore bounded by 256 KiB + 1024 * size_of(ParsedElement) + 1024 * 64 bytes, plus fixed receiver/ParsedFile overhead and allocator metadata. The host layout probe (`layout.txt`) measures ParsedElement=176 bytes and ParsedFile=48 bytes, giving a 496 KiB logical payload envelope before fixed holder overhead; allocator capacity/RSS can differ. Fresh capture/parse temporaries retain their previous per-file behavior; this is an additional reuse-state bound, not a process-RSS limit. Search-result metadata/ranking allocations already present at baseline are unchanged.

## Tests and work counts

Tests use the real MarkdownParser with an operation counter and a scripted FileReader boundary returning real CapturedFile values. File metadata deliberately stays identical across different bytes, so the edit regression catches accidental stat-based reuse. Expected flag sequences and counts are independent literals; parser-derived metadata is only the pinned initial fixture.

Initial RED receipt `red.log`: all three new tests failed for exactly the missing work reuse. Sixteen captures across two eight-binding operations invoked the baseline parser 16 times instead of the expected 2; two equal-content paths invoked it 4 instead of 2 times; the edit/invalid/missing/restored sequence invoked it 7 instead of 6 times. No compile-error RED is claimed.

Initial GREEN receipt `green.log`: eight tests passed. The changed/current/invalid/missing/restored sequence preserves stale flags `[false,true,false,true,true,true,false,false]`, performs eight fresh captures and six parses. Repeated unchanged data performs sixteen captures but only two parses across independent operations. Distinct paths perform four captures and two parses. Byte, element and summed path/ID budget overflows each perform two ordinary parses for two bindings, preserving current flags. A valid→missing→same-valid sequence reparses restoration. The per-binding hash/line test keeps mismatched pinned metadata stale even after reuse. Retained-payload and late capture-I/O/retry tests cover payload release and state lifetime.

Final hand-mutation results, restored-tree verification, and binary manifest are recorded below.

## Performance hypothesis and limits

The complete-coverage baseline supplied by the coordinator is 111.853 s median with overlapping inclusive native scopes: eligibility 96.91%, parsing 53.64%, historical hashing 20.02%, current-source hashing 8.40%, vector lookup 1.90%. Last-file reuse should reduce repeated full-file parsing when many eligible candidates share a file. It adds one whole-file SHA-256 per current Markdown capture. Every live capture, provenance observation, filter decision and ranking input remains in place. End-to-end improvement is a hypothesis until the coordinator runs matched ordinary CLI trials against all 100k stored vectors; no speedup or minimum-hardware acceptance is claimed here.

Last-file policy deliberately provides no retention benefit when distinct paths interleave. Extremely large files/element sets/ID sets also bypass. Parsing remains whole-file atomic. Equal SHA-256 identity is the same content-identity assumption already used by the product; there is no stat-only shortcut. The actual filesystem confinement/stability implementation is unchanged; scripted between-capture tests exercise the service boundary and do not replace adapter tests.

## Architectural review

Application orchestration and the bounded state stay private in the existing flat service implementation, whose inline tests remain last. Collaborators are constructor/operation injected and accessed through the receiver. The dependency-free domain and technology adapters are unchanged. The common eligibility rule remains owned by service/utils; no duplicated matching predicate, wrapper service, new public value facade, or DI abstraction was added. Physical paths, visibility and import direction were reviewed against AGENTS.md and the architecture program-structure sections. X-4's deterministic path+bytes MarkdownCodec contract permits this pure-work reuse after each fresh capture. MC-6's complete eligible set, source/revision same-binding semantics, ranking/ties and full native K are unchanged.

## Verification environment and completed stages

Host: Darwin arm64, pinned rustc 1.95.0 (59807616e, LLVM 22.1.2). All Cargo builds use only this experiment's own `target/` directory and `CARGO_BUILD_JOBS=2`; no shared/root target is used or copied.

The final source uses borrowed `dyn FileReader` and `dyn MarkdownCodec` only in the new operation receiver, preserving existing service generics. The full verification command is `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=../target CARGO_PROFILE_RELEASE_DEBUG=1 RUST_TEST_THREADS=2 make verify V=1` from `source/`, captured in `verify.log`. Completed stages: fmt, all-target check, all-target Clippy with denied warnings, nine fixed-value-lint tests and executable analysis, full debug root suite (1206 passed, zero failures, 11 ignored), strict Rustdoc. Release root suite also passed: 1205 passed, zero failures, 11 ignored; make verify ended with `ok`, exit 0. Exact counts are in `verify-summary.json`. No Linux/native storage runner or historical standalone proof harness is claimed; this change touches neither storage/native/build/vendor inputs nor Git behavior.

## Final receipts and handoff

Final exact-source mutation rerun: all ten viable mutants caught by test failures (not compile errors), `mutation-run.log` / `mutations.json`, exit 0. Covered removal of fresh capture, content-hash matching, path separation, per-binding predicate evaluation, missing-state eviction, each independent budget, search-text release and graph release. Earlier dead-code-unviable formulations remain in `*.attempt1.*`; five of that first set were unviable rather than test escapes. Corrected prior generic-receiver receipts remain in `*.pre-dyn.*`. The final run uses the delivered borrowed-dyn implementation. All source bytes were restored and match the manifest of the full successful verify run; the final scoped eight-test run passed again (`final-green.log`).

Standalone production build: `CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=../target CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --locked --offline`, exit 0, optimized + debuginfo, 39.21 s (`build-release.log`). No new broad test suite was started after the successful full verification.

Pinned executable: `bin/letopis`, 11117176 bytes; SHA-256 `7106a460e5177bfe1472b543840908ec69f684e1826e9b40823d5f37050eeb7a`. Source `src/service/search.rs` SHA-256 `94a1c8473134b681fa36690368da520bff9fab2d241093775e9fc5d6939f7c41`. Patch SHA-256 `a9fd5947e0db0d9eb7972d3b1eb4510435b84fbb9386d3d39518fa5e0dd5c4a8`. The binary returns `letopis 0.2.0`. `binary-manifest.json`, `source-manifest.json` and `restoration.json` bind the baseline, patch, source, binary and final mutation state. `git diff --check` and reverse patch applicability pass.

No background work remains. Coordinator may begin serial whole-corpus measurements. Runtime speedup, full-coverage output/count oracles and matched peak RSS remain the coordinator screening scope; no such measurements are claimed by this candidate report.
