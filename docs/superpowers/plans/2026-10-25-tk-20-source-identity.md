# TK-20 Source Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete derived source identities and current availability, preserving opaque remote addresses without fetching them.

**Architecture:** Extend pure source addressing, reuse atomic document projection, and introduce a synchronous source reader over a caller-owned pinned storage view and confined filesystem. `lib.rs` constructs the concrete service; CLI source inspect stays TK-24, historical provenance stays TK-21.

**Tech Stack:** Rust 1.95.0, existing rusqlite/SeaQuery/SQLite, pulldown-cmark, synchronous filesystem and Git adapters; existing Python/jsonschema for contract checks. No new dependency.

**Spec:** [Approved specification](../specs/2026-10-25-tk-20-source-identity-design.md), approved by the user on 2026-10-25.

## Global Constraints

- Worktree `/Users/zdkadiev/.codex/worktrees/tk-20/letopis`, branch `tk-20`, base main `a5aa3029521fb97164114047c13b0e4b74ec9232`; planning starts after documentation commit `3672b4c`.
- HTTP/HTTPS URLs are separate opaque addresses: preserve spelling, query and fragment; no equivalence/canonicalization, network fetch, or local-file interpretation.
- Local paths use existing TK-7 root-relative normalization; ranges are one-based, inclusive; exact bytes, no UTF-8 decoding or EOL normalization.
- A source exists while effective external links reference it; creation time is the minimum authored referencing knowledge timestamp; invalid files retain their last valid projection.
- The source reader accepts the caller's existing pinned `StorageRead`; no refresh, storage reopening or publication inside that read. Live file observations are not a shared Git snapshot.
- Explicit cache rebuild on upgrade, no automatic migration/deletion. Preserve Markdown, Git, configuration, locks and `vectors.db`; development only removes disposable fixture caches after all their handles close.
- No CLI source inspect, source-revision filters, historical comparison, new source registry/schema, web fetch, Tokio, process or paid model run.
- Follow [AGENTS.md](../../../AGENTS.md) and [architecture review](../../zkit/letopis/architecture.md#program-structure-review): physical modules, service-owned traits, real constructor wiring, documented public Debug types, verbs/get_ accessors, explicit imports, typed safe errors and focused functions (about 40 lines, upper limit 60).
- Tests use Arrange/Act/Assert and independent literals/oracles. Mutation controls are selected manual changes with exact restoration; no automatic whole-crate campaign.
- Keep source bodies out of storage/results; retain at most one source file's capture per observation/group. Keep bound ordinary SQL inside storage.
- Baseline `make check` already passed; do not repeat it for this documentation-only planning turn. During implementation run focused checks per task and final `make verify` on the actual completed tree.
- Shared Engram only: `ENGRAM_DB=/Users/zdkadiev/Repos/letopis`. Preserve existing graph attributes and other writers' changes. G-1/resource/pilot and later task acceptance remain unassessed.

## Review Focus

1. `HTTP://Host/a?x=%2f#L01-L02` must preserve every character, remain unfetched and never become a local path; Task 1/2 tests.
2. Requesting an unknown source or a knowledge ID through the source reader must return no source and perform zero file reads; Task 2 tests.
3. Source bytes change or disappear while referrer Markdown is unchanged: the next explicit read observes live hash/absence, not the cached hash; Task 2 tests.
4. An empty file and a trailing newline must not manufacture an addressable extra line; CRLF/binary bytes remain exact; Task 1/2 tests.
5. Rebuilding a legacy cache must repair URL identity without touching canonical files or vectors; invalid referrers must retain their effective minimum timestamp until repair/removal; Task 3 tests.

---

## File Structure and Responsibilities

| Files | Responsibility |
| --- | --- |
| New `src/domain/source.rs`; modify `src/domain/identity.rs`, `mod.rs`; narrowly extend derives in `value.rs` | Move SourceUri to its own owner; classify opaque remote/local addresses and select local bytes. Leave KnowledgeId/RepoPath/Anchor behavior unchanged. |
| Modify `src/service/mod.rs`; new `src/service/source.rs`, `src/service/source/tests.rs` | Application-owned source reader/result types, concrete SourceService and private behavioral tests. |
| Modify `src/service/projection.rs`, `model.rs`, `src/lib.rs` | Reuse addressing for publication, document remote metadata, wire actual source reader constructor. |
| New `tests/source_addresses.rs`, `source_projection.rs`, `source_reading.rs`, `source_lifecycle.rs`, `test_source_contract.py` | Separate pure-address, publication, public-service, lifecycle/upgrade and schema behaviors. Reuse existing test support; new helpers stay private unless genuinely shared. |
| Modify X-1 schema, X-4 and I-4 contracts, mechanics MC-4, README/development guide and TK-20 card | Record approved URL semantics, consumer ownership and explicit upgrade procedure; preserve historical receipts. |
| New `docs/zkit/letopis/checks/TK-20/report.md`, `verification.json` | Captured verification scope, inputs, outcomes, review and remaining items. |

No changes to SQL schemas, native build, vendors, providers, CLI handlers or existing historical proof harnesses are planned. If execution finds an actual need, explain the contract gap before broadening the change and run the affected native runner.

## Task 1: Typed source addresses and shared exact-byte rules

**Files:** `src/domain/source.rs` (new), `identity.rs`, `mod.rs`, narrowly `value.rs`; `tests/source_addresses.rs` and `tests/test_source_contract.py` (new); `docs/zkit/letopis/contracts/schemas/X-1-cli.yaml`.

**Interfaces:**
- Consume existing `RepoPath::parse`, `LineRange::new`, `ValueError::{InvalidPath, InvalidRange}`.
- Keep public `SourceUri::parse(raw: &str) -> Result<Self, ValueError>` and `as_str(&self) -> &str`.
- Add `SourceUri::get_local(&self) -> Option<&LocalSource>`; remote returns None, local is already validated, no reparsing in callers.
- Add public, privately-fielded `LocalSource` with `get_path(&self) -> &RepoPath`, `get_lines(&self) -> Option<LineRange>`, `select_bytes<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]>`.
- Keep SourceUri's existing Debug/Clone/Eq/Ord behavior. Store the canonical address plus private optional LocalSource; derive ordering on LocalSource/LineRange only as needed by existing SourceUri map keys. Do not expose mutable address internals.

- [x] **Write failing address/byte tests.** Name the tests `normalize_local_source_identity`, `preserve_opaque_remote_identity`, `reject_malformed_source_addresses` and `select_exact_source_lines`. Representative literal assertions (each actual test uses its own Arrange/Act/Assert phases):

```rust
assert_eq!(SourceUri::parse("./src/../src/a.rs#L010-L025").unwrap().as_str(), "src/a.rs#L10-L25");
let remote = SourceUri::parse("HTTP://Host/a?x=%2f#L01-L02").unwrap();
assert_eq!(remote.as_str(), "HTTP://Host/a?x=%2f#L01-L02");
assert!(remote.get_local().is_none());
assert!(SourceUri::parse("https://").is_err());
assert!(SourceUri::parse("https:/host/a").is_err());
assert!(SourceUri::parse("../outside").is_err());
let one = SourceUri::parse("a#L1-L1").unwrap();
assert_eq!(one.get_local().unwrap().select_bytes(b""), None);
assert_eq!(one.get_local().unwrap().select_bytes(b"\xff\r\n"), Some(&b"\xff\r\n"[..]));
let two = SourceUri::parse("a#L2-L2").unwrap();
assert_eq!(two.get_local().unwrap().select_bytes(b"x\n"), None);
assert_eq!(two.get_local().unwrap().select_bytes(b"x\ny"), Some(&b"y"[..]));
```

Also pin whole empty bytes as `Some(b"")`, incomplete ranges as None, root escapes/controls/overflow/reversal rejection, case/Unicode/percent spelling preservation, and distinct whole-file/range identities. HTTP scheme comparison is ASCII case-insensitive; original text remains unchanged.

- [x] **Run RED:** `cargo test --locked --offline --test source_addresses --quiet`; new accessors are absent and existing URL preservation assertions fail. Record the actual intended failure before coding.
- [x] **Implement the interfaces.** Move only SourceUri out of identity.rs, keeping root reexports stable. Recognize reserved HTTP/HTTPS scheme before RepoPath normalization; require `://` and nonempty authority, reject controls/whitespace/backslashes in remote input, keep remaining URL text opaque. Do not infer line ranges from URL fragments or add a URL dependency. Copy local normalization rules and exact-byte selection into the owning type; do not change local path grammar to match URLs.
- [x] **Align the existing SourceId schema.** Give SourceId separate local and opaque HTTP/HTTPS alternatives, with the local alternative excluding reserved HTTP schemes so malformed remotes cannot fall back to it. Keep semantic range ordering and full URL authority checks in the application; document the schema's lexical limit. No new commands or output fields. Add `tests/test_source_contract.py` using `json.loads` and prepared `jsonschema.Draft202012Validator`, testing `$defs.SourceId` directly: local path/range and remote fragment accepted; controls, absolute local path, empty remote authority and malformed HTTP slash rejected. This validates v1 address spelling, not CLI source inspect implementation.
- [x] **Run GREEN:** focused Rust address tests plus existing `cargo test --locked --offline --test domain_values --test markdown_elements --test markdown_links --test markdown_search --quiet`. Run the Python contract target below.
- [x] **Mutation-check and restore:** make remote input pass through local slash normalization, then make a trailing newline create an extra line. Confirm each relevant test fails independently; restore exact source hashes and rerun affected tests.
- [x] **Commit only Task 1 paths:** `feat: distinguish local and opaque remote source addresses`.

## Task 2: Reconcile remote nodes and expose current source observations

**Files:** `src/service/mod.rs`, `model.rs`, `projection.rs`, `src/lib.rs`; new `src/service/source.rs`, `src/service/source/tests.rs`, `tests/source_projection.rs`, `tests/source_reading.rs`.

**Interfaces:**
- Consume Task 1 SourceUri/LocalSource methods, existing `FileSystem::read_file`, `StorageRead::read_node`, `SourceMetadata`, `DocumentResult`, `NodeMetadata` and `EdgeTarget`.
- Define small public interface/result declarations in `service/mod.rs`:

```rust
pub trait SourceReader {
    fn read_source<R: StorageRead>(&self, view: &mut R, uri: &SourceUri)
        -> DocumentResult<Option<SourceObservation>>;
}
pub struct SourceObservation {
    pub metadata: SourceMetadata,
    pub availability: SourceAvailability,
}
pub enum SourceAvailability { Available, SourceNotFound, Unfetched }
```

All public declarations implement/document Debug as appropriate. `SourceNotFound` is the typed `source_not_found` reason for a referenced local address; None means no source node, and Unfetched means remote availability is unknown. Do not create a separate redundant flag collection or change CLI SearchFlag serialization. Derive Debug/Clone/PartialEq/Eq for the observation and Debug/Clone/Copy/PartialEq/Eq for the availability enum.

- Concrete crate-private `SourceService<F: FileSystem>` and `new(filesystem: F) -> Self` live with their implementations in `service/source.rs`; reexport only crate-private constructor access.
- Public composition `wire_source_reader(root: &Path) -> DocumentResult<impl SourceReader + use<>>` in lib.rs takes the caller's already-selected Git root and creates LocalFileSystem then SourceService. It does not discover/open storage, create `.letopis`, refresh, pin Git trees or launch a drain. The opaque composition return follows existing lib.rs constructors.

- [x] **Write failing publication tests** using real Markdown/refresh/SQLite. Two external links to equivalent local ranges produce one source with minimum time 3. A link to `https://Host/a?x=1#section` persists that exact ID with hash/lines None. An instrumented filesystem delegates Markdown/local reads but records/asserts that remote addresses cause zero filesystem reads. Existing local grouping still reads one file per affected path. Knowledge/FTS remain available for remote referrers.
- [x] **Write failing reader tests.** With real SQLite plus instrumented FileSystem, assert unknown source and a stored knowledge at the same string return None with zero reads; referenced remote returns `(None, None, Unfetched)` without reads; referenced missing/range-short local returns SourceNotFound with hash None; available local returns exact bytes' hash and addressed lines. For whole-file `b"abc"`, use literal SHA-256 `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, not production hashing as the expected value.
Name the reader tests `skip_io_for_unknown_or_knowledge_address`, `return_unfetched_remote_without_io`, `observe_live_source_without_referrer_edits` and `propagate_private_source_failure`. In their arranged fixtures, keep these assertions visible:

```rust
assert!(reader.read_source(&mut view, &unknown).unwrap().is_none());
assert!(recorded_reads.borrow().is_empty());
let observed = reader.read_source(&mut view, &remote).unwrap().unwrap();
assert_eq!(observed.availability, SourceAvailability::Unfetched);
assert_eq!(observed.metadata.hash, None);
assert_eq!(observed.metadata.lines, None);
assert!(recorded_reads.borrow().is_empty());
```

- [x] **Add live-observation and pinned-membership scenarios.** Keep Markdown unchanged while source bytes change, disappear and return; literal expected hashes/states change on each explicit read. Pin an old cache read, remove the last reference through another writer, and prove old-view membership remains while a new view returns None; live file bytes remain independently current. Source reading must not alter cache revision, vector identity or source_snapshot.
- [x] **Add failure tests.** A filesystem error with a secret private cause must remain an error with safe public text, never SourceNotFound; a broken/failed storage read propagates similarly. Symlink/nonregular source reads fail through the existing confined adapter. Returned observations never contain bytes. Keep test hooks private; use real storage corruption or a private delegating read wrapper for failure injection.
- [x] **Run RED:** `cargo test --locked --offline --test source_projection --test source_reading --quiet` and `cargo test --locked --offline --lib service::source::tests --quiet`; verify absent reader symbols or incorrect current remote publication causes the expected failure.
- [x] **Implement SourceService::read_source.** First read the requested source node from the supplied view. None or a knowledge node returns None before any file capture. For a matching SourceMetadata row, retain derived URI/creation time, replace hash/lines with the current observation using LocalSource; remote returns Unfetched without I/O. Propagate errors using existing typed DocumentError conversion and safe static messages. Do not trust cached hash/lines for live bytes, reopen a read, publish, or read all sources.
- [x] **Update EffectiveGraph::reconcile_sources.** Preserve existing reference/minimum/orphan computation and transaction boundaries. Separate remote sources before path grouping, write their metadata with hash/lines None; local groups use get_local/get_path/get_lines/select_bytes. Remove the old private parse_source_path, parse_source_lines and select_source_bytes duplicates. Keep a shared local capture until all affected ranges for that path are processed, then release it.
- [x] **Wire and document the constructor/types.** lib.rs must actually call LocalFileSystem::open and SourceService::new. SourceMetadata docs distinguish unknown remote bytes from missing local bytes. Do not connect CLI inspect or add command scaffolding.
- [x] **Run GREEN:** reader/projection/library source tests and `cargo test --locked --offline --test document_refresh --test storage_cache --test knowledge_inspection --quiet`; then `make check`. Inspect imports/visibility/wiring and function sizes, beyond lint success.
- [x] **Mutation-check and restore:** bypass membership before I/O, return the persisted hash for a live local read, and route a remote source into file capture. Each corresponding test must fail independently; restore exact hashes and rerun affected tests.
- [x] **Commit only Task 2 paths:** `feat: read current source availability through application interfaces`.

## Task 3: Prove lifecycle/rebuild behavior and finish the handoff

**Files:** new `tests/source_lifecycle.rs`, `docs/zkit/letopis/checks/TK-20/report.md`, `verification.json`; focused updates to tests/cli/README.md (root README.md is absent), docs/development.md, mechanics MC-4, contracts X-4/I-4, and the TK-20 task card. Update this plan and the specification's delivery status without rewriting their accepted behavior.

**Interfaces:** Consume Tasks 1/2 public address/reader APIs plus existing `wire_document_refresher`, `open_storage`, CacheWrite/StorageRead/VectorWrite. No new product interfaces.

- [x] **Write lifecycle regression tests.** Literal expected source ID `source.txt#L2-L3`, times 3 then 8 after earliest referrer removal; invalid remaining referrer retains time 8 and its effective edge; removing its file removes the source, then new referrer time 11 recreates it. Cover whole-file/range distinction, duplicate external links, source disappearance without knowledge deletion, and existing source/knowledge collision rollback with unchanged FTS/revision.
- [x] **Write clean rebuild test.** Capture expected source IDs/minimum times, close every fixture connection/refresher/view, remove only fixture cache.db and present sidecars, refresh through the existing service and assert the same expected identities/times. Leave vectors/config/Git/Markdown/lock files in place. No live or repository-owned cache removal.
- [x] **Write legacy upgrade test.** Seed a disposable cache with the old-parser identity `https:/Host/a` using bound fixture SQL only (not a new production migration). Keep original Markdown URL `https://Host/a` unchanged. Preserve a durable vector `[1.0, 0.0, 0.0]` and failure/attempt sentinel under a known existing vector identity using Storage/VectorWrite. Close connections, apply the explicit cache-only fixture rebuild, refresh, assert exact URL identity and no legacy node, unchanged Markdown/Git/config/embed.lock contents; index.lock inode retained and its bytes unchanged across deletion, while normal refresh may rewrite the owner PID, and the same vector/attempt values. Do not assert automatic compatibility before rebuilding.
Use names `derive_minimum_from_effective_references`, `rebuild_source_identity_from_canonical_files` and `repair_legacy_url_only_after_explicit_cache_rebuild`. Assert the independently declared expected IDs/times and preserved vector literal, for example:

```rust
assert_eq!(source.uri.as_str(), "source.txt#L2-L3");
assert_eq!(source.created_at, 8);
assert_eq!(remote.uri.as_str(), "https://Host/a");
assert_eq!(preserved_vector, Some(vec![1.0, 0.0, 0.0]));
```

- [x] **Run lifecycle tests before fixes:** `cargo test --locked --offline --test source_lifecycle --quiet`. These integration proofs may already pass after Tasks 1/2; do not manufacture a RED baseline or duplicate implementation. Where they expose a real gap, preserve the failing receipt and fix only its owning behavior before continuing.
- [x] **Mutation-check lifecycle tests and restore:** substitute maximum for minimum referencing creation time, and suppress orphan-source deletion. Confirm focused tests fail; restore exact files and run the focused suite again.
- [x] **Document the upgrade and accepted contracts.** Stop all repository Letopis/drain processes first; rebuild only derived cache.db plus its quiescent SQLite sidecars, preserving vectors/config/locks/canonical files. Explain that current `check`/refresh recreates it; CLI reindex is not implemented here. Record opaque remote addresses/no-fetch/unknown bytes in X-4, I-4 and MC-4 with a dated link to the approved specification. Clarify I-4 historical methods remain TK-21; no new public CLI fields. Do not edit captured historical evidence or mark G-1/G-2 passed.
- [x] **Run final verification on an unchanged input tree.** Use the commands below, capture logs and inventory with Rust/tool/platform versions. If storage/native/build/vendor product files changed unexpectedly, additionally run the relevant host storage runner in a new directory and report exactly which platform/harness was executed. Root suites do not imply historical harness or Linux execution.
- [ ] **Perform final review.** Verify spec coverage, physical module/API/import direction, constructor wiring, URI/schema parity, no remote I/O, no stored source bytes, typed missing/error distinction, selected controls/restoration and upgrade limits. Execution method determines whether per-task reviews precede this whole-branch review; do not rerun unchanged full suites only for documentation edits.
- [ ] **Complete report/card/receipt.** Map each TK-20 criterion to evidence, distinguish service-level acceptance from pending CLI consumers, include known unchanged ignored tests/platform limits. Record outcomes in shared Engram preserving attrs. Update GitHub results when authorized; merge/push/closure require their existing concrete authorization, not a fabricated completion.
- [x] **Commit intended Task 3 paths:** `test: verify source lifecycle and document explicit cache rebuild`.

## Commands and Evidence

Run from the task worktree. Export `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target` and `RUST_TEST_THREADS=2` for Rust checks. Prepare no new dependencies during checks; if genuinely absent, fetch locked dependencies separately before verification.

Prepared Python (verify it is still available before use):
`/Users/zdkadiev/.codex/worktrees/tk-11/letopis/.artifacts/tk11/python-env/bin/python` (3.12 + jsonschema 4.26.0).

```sh
cargo test --locked --offline --test source_addresses --quiet
cargo test --locked --offline --lib service::source::tests --quiet
cargo test --locked --offline --test source_projection --test source_reading --test source_lifecycle --quiet
/Users/zdkadiev/.codex/worktrees/tk-11/letopis/.artifacts/tk11/python-env/bin/python -B tests/test_source_contract.py
make check
make verify
```

Commands name planned new targets; none has been run or claimed passing during planning. Before final verification, apply formatting with `make fmt` if needed. Use normal compact success output; write full failures/evidence to new `.artifacts/tk20/<task-or-final>/` paths, never replace historical receipts.

Verification JSON records implementation revision plus dirty status (or exact source inventory), platform, command exits, input SHA-256 map, evidence paths, control outcomes and exact restoration. Report final reviewed commit and distinguish tested implementation inputs from later documentation-only completion commits.

## Self-Review and Handoff

Self-review checklist performed while preparing this plan:

- Spec coverage: local/remote identity → Task 1; derived publication/current metadata/errors → Task 2; lifecycle/rebuild/upgrade/docs/evidence → Task 3.
- Interfaces: LocalSource/SourceReader/result types are defined before use; constructors keep actual composition in lib.rs; Storage/FileSystem ownership is unchanged.
- Review focus: each of the five named conditions has an explicit owning test above.
- Scope/proportion: three independently reviewable deliverables, no product bodies copied into this plan, no new CLI/historical provider behavior.

Status: approved by the user on 2026-10-25; subagent-driven execution explicitly selected. Three sequential implementation tasks, independent task review after each, then whole-branch review. Execution state and rulings are recorded in `.superpowers/sdd/2026-10-25-tk-20-source-identity/progress.md`; acceptance checkboxes are checked only after the corresponding work and review pass.

## Task 3 implementation handoff — 2026-10-25

Six lifecycle/regression fixtures pass, including explicit legacy-URL cache rebuild
and preserved vector/attempt/failure state. Both selected lifecycle controls fail
as intended and restore exact projection bytes. Final focused suites: 23 Rust and
2 Python tests passed. Single `make verify V=1` passed: debug/release each 548 passed,
5 ignored over 39 root binaries; nested child summaries are excluded by the exact
counting method in the [report](../../zkit/letopis/checks/TK-20/report.md).
All 291 inventoried verification inputs are unchanged before/after. Native storage
runner is not required (storage/native/build/vendor product inputs unchanged).
The root README path assumption is corrected to existing tests/cli/README.md;
index.lock assertions preserve identity and deletion-time bytes while allowing
normal MC-5 ownership PID updates. Independent Task 3/whole-branch review and
shared Engram handoff remain controller work; corresponding boxes stay unchecked.
