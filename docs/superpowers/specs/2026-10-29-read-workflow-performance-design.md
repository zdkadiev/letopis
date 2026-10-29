# Read workflow performance

**Status:** revised after the user decision on 2026-10-29: full-corpus `check` and `reindex` perform batched full source-snapshot verification; routine reads avoid full source-closure walks. This supersedes the earlier batching-only proposal. The user authorized implementation and all-command E2E on 2026-10-29. Implementation follows this accepted boundary; measured results and verification are recorded separately.

## Goal and accepted scope

Optimize `search`, `traverse`, and `inspect` by separating routine retention/addressed-byte integrity from full source-snapshot verification. Preserve automatic freshness, correct addressed bytes and atomic publication; explicitly move exhaustive detection of unrelated source-object damage to full-corpus `check` and `reindex`. Markdown remains canonical; commands refresh the database from files and never edit authored Markdown. The user requested actual Rust profiling before choosing the optimization, plus a written spec and plan.

Keep the synchronous Rust implementation, Git CLI boundary, SQLite transaction model, current command shapes, response fields, and errors. No watcher, async runtime, persistent verification cache, new CLI configuration, or freshness mode is introduced.

Sources: [architecture](../../zkit/letopis/architecture.md#program-structure), [mechanics](../../zkit/letopis/mechanics.md), [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md), [X-3 Git](../../zkit/letopis/contracts/X-3-git.md), and [TK-26 corruption guarantees](2026-10-26-tk-26-corruption-design.md).

## Profile before implementation

Capture each command's request, response, exit code, elapsed time, binary hash, build profile, repository revision, platform and Git version. Use macOS `sample` with resolved Rust stacks; Instruments is unavailable on this host. Collect Git invocation counts separately through a PATH wrapper executing the original Git binary. Label wrapper-instrumented timings separately. Sampled child-resource totals include profiler activity and are not pure product CPU measurements.

Use an equivalent owned trial fixture and published cache. The original `knowledge/architecture.md` was removed by the user after the historical profile; do not recreate it in the main checkout or treat stale cache entries as a fresh trial. Do not silently initialize a second Engram store or change embedding configuration. Profile local FTS behavior separately from provider inference. Record initial and final cache status and whether the publication revision changed. Preserve raw evidence in a fresh ignored `.artifacts/` directory and summarize findings in a tracked report. Do not treat a short sample window as a percentage of the complete command.

The known 56.84/57.29/57.71-second dev-build timings are exploratory observations, not a release benchmark. Compare before/after with the same build settings, corpus and requests; obtain optimized-build measurements before claiming production latency.

## Verified current behavior

- The three services call `refresher.refresh()` before their main read operation.
- `DocumentRefreshService::refresh` avoids its lock only when there are no changes **and no source snapshots**. With snapshots, even unchanged refresh calls `retain_snapshots` under `index.lock`.
- `ProvenanceService::retain_snapshots` calls `GitSnapshots::pin` for available roots.
- `Retainer::pin` verifies the entire closure before inspecting or publishing its direct retention ref.
- `verify_closure` walks trees and blobs. Individual verification starts Git for metadata, content, and independent rehashing. Trees also use `ls-tree`.
- `inspect` additionally captures current corpus/source bytes and returns a newly retained reading snapshot. Preserve this behavior; it is distinct from an author's `source_snapshot`.

Fresh five-second Rust samples for all three commands stay in retention and closure verification, including child-process waits. Complete dev-build timings were 58.196 seconds (search), 60.194 (traverse), and 56.453 (inspect). A separate wrapper-counted search launched 6,258 Git subprocesses, including 3,946 cat-file, 1,973 hash-object and 294 ls-tree. Instrumented timings, corpus changes and other limits are recorded in the [baseline report](../../zkit/letopis/checks/read-workflow-performance/2026-10-29-baseline.md); optimized-build matched baselines remain an implementation gate.

## Accepted verification boundary

| Operation | Cache refresh | Verification of author-declared source snapshots |
| --- | --- | --- |
| `search`, `traverse`, `inspect` | Keep automatic incremental refresh | Root/ref safety plus integrity of objects actually read; no exhaustive source-closure traversal |
| `check` with omitted or empty `paths` | Incremental refresh | Full closure verification of every effective source snapshot, batched, before any new SQL publication |
| `check` with nonempty `paths` | Keep current whole-corpus refresh and filtered findings | Routine retention and addressed-byte checks; no exhaustive audit, even if the supplied list happens to name all current files |
| `reindex` | Full reparse/rebuild | Full closure verification of every effective source snapshot, batched, before SQL publication |
| `status`, `config`, help | Existing observation/help behavior | No new full verification or mutation |

An author's `source_snapshot` remains an author's declaration of the checked source basis. A service ref preserves reachability under normal Git garbage collection; it is **not** a certificate that every descendant is intact. Routine refresh may retain a newly declared existing tree after checking its root without validating unrelated descendants. The first complete audit can therefore happen later at full `check`/`reindex`. Never mark such a snapshot as fully verified merely because its ref exists.

Corruption of a root or an object actually needed by a read still fails with safe typed diagnostics. Corruption of an unrelated descendant need not fail search; it must fail the next full audit, including after reopening and with the retention ref already present. Thus the timing of exhaustive detection changes deliberately; do not claim the old every-read full-integrity guarantee is unchanged. Missing author roots retain their existing current-diagnostic semantics rather than being converted into fatal corruption errors.

`inspect` still captures current knowledge and addressed source bytes into its own reading snapshot and verifies that new, bounded-by-captured-files tree before returning it. This is separate from traversing an author's potentially repository-wide source tree. Reusing unverified old metadata or returning live bytes inconsistent with the reported hash is forbidden. Large requested captures can still take proportional time; 'fast' excludes exhaustive unrelated source scans, not the cost of requested data.

## Proposed implementation

Separate the Git and application-owned capabilities explicitly:

- Keep `GitSnapshots::pin` strong: verify the entire local closure and confirm a direct retention ref. Its public corruption tests remain valid.
- Add `GitSnapshots::retain` for root-validated ref retention without a closure walk. It returns `Result<(), SnapshotError>`, not `RetainedTree`, whose existing type documents a fully validated closure.
- Make `BasisRetainer::retain_snapshots` use the lightweight operation; add `verify_snapshots` for the strong batched operation. Both deduplicate roots and require index.lock at the existing publication boundary. Preserve the separate policy rejecting alternate stores and lazy fetch.
- Keep `Refresher::refresh` lightweight. Add `refresh_verified` for incremental refresh with exhaustive verification, and retain `reindex` for exhaustive verification plus full reparse. Internally separate publication mode from verification scope with one small private enum, rather than pretending that full check is a reindex.
- `CheckService` chooses `refresh_verified` only for empty `paths`; otherwise it calls `refresh`. Perform required full verification under index.lock before the SQL write phase, even when the file delta is empty. No additional 'refresh, then audit another revision' sequence is introduced.
- Full `check` must compare the verified refresh outcome's revision with the revision pinned by its subsequent storage read. If another publisher advances the cache before that read begins, return a safe `internal_error` explaining that the corpus changed during check and the caller should retry; do not issue a clean report for an unaudited revision. No unbounded retry loop or longer-lived lock is added. After the view is pinned, later publications do not invalidate that immutable read view. Source-only live changes remain explicit observations, not a claim of cross-file filesystem atomicity.
- Search and traversal continue through ordinary refresh. Inspection continues through ordinary refresh and strongly pins only its newly assembled reading snapshot. No new CLI flag, storage table, verification-status bit, or global process pool is needed.

Preserve exact direct-ref checks, symbolic/conflicting-ref rejection, compare-and-swap publication and failure on ref errors in both retention paths. Validate an available root before publishing its ref; do not invent missing bytes or silently replace a wrong ref. The full audit covers roots referenced by the resulting valid projection, including last-valid elements retained for invalid Markdown. Removed/replaced valid elements do not keep obsolete roots in the audit set; compute that metadata-only prospective set before SQL writes and recheck capture consistency as today. Test both invalid last-valid retention and replacement/removal of old roots. A failed full audit leaves the prior graph/FTS publication intact; authored Markdown and user Git state are not changed. Retention refs successfully created before a later failure may remain, as today; they are not proof of completed verification. This is a consistency guarantee, not a claim of cross-resource atomicity or physical power-loss durability.

Replace closure verification's per-object process launches with one private `git cat-file --batch` session per `pin` call. This session is short-lived and owned by the operation, not a global pool. Traverse only objects reachable from the requested root and deduplicate within that operation; skip gitlinks as today. Read raw tree entries from the verified tree bytes instead of starting `ls-tree` per directory.

Structural-validation correction from independent review: the previous hash-object command also ran Git's strict tree checks. Preserve those exact checks with one additional `git hash-object -t tree --no-filters --stdin-paths` process per closure. Spool only verified tree bodies into an operation-owned private temporary directory (0700, files 0600), C-quote input paths, compare every returned OID and clean up on success/error/drop. Blob bodies remain streamed and are never spooled. This keeps process count constant without reimplementing Git's HFS/NTFS path aliases, ordering, duplicates, modes and special-file policy. It adds transient disk I/O. The decoder's normalization oracle does not widen strong-pin acceptance: modes such as raw 100664/100600 are still rejected by Git's existing strict validator. The earlier claim that ls-tree normalization alone represented the old acceptance policy was incomplete. Sources: [Git hash-object structural checks](https://github.com/git/git/blob/v2.50.1/object-file.c) and [tree fsck](https://github.com/git/git/blob/v2.50.1/fsck.c).

Use the protocol available at Git 2.29: send full hexadecimal OIDs followed by LF; consume a bounded header, exactly the declared number of body bytes, then the trailing LF. Do not use newer `--batch-command` or `-Z` options. Match the response OID and expected type, validate sizes and complete framing, and fail closed on missing children, malformed data, premature EOF, failed writes, or child failure. Preserve the existing root-versus-child error classification.

Recompute the Git object ID over `<type> <decimal-byte-length>\0<body>`. Stream blob bytes through a bounded buffer; retain only the current tree's bytes and the traversal's OID/type bookkeeping. Do not accumulate all closure contents. Parse raw tree modes/names/OID widths, including SHA-1/SHA-256, symlinks and gitlinks; retain Git structural validation before success. Detect conflicting expectations for a shared OID. Test decoder mode normalization independently from strong pin acceptance: fixtures must include raw `100664` and `100600` normalized to `100644`, plus executable, symlink, tree and gitlink cases. Git's `canon_mode` and tree decoder are the decoder compatibility reference; the final batched Git validator preserves the existing strict structural policy.

**Proposed dependency choice:** retain `sha2 = "=0.10.9"` for SHA-256 and evaluate `sha1-checked = "=0.10.0"` for collision-detecting SHA-1. Its documented MSRV is 1.72; our pinned Rust is 1.95.0. Its docs.rs build failed, so a compile/API probe is a mandatory pre-implementation dependency gate. A failure requires revising this choice, not substituting unchecked SHA-1 or silently adding a runtime. Dependency changes and the lockfile require approval as part of this proposal.

Reuse Git command environment hardening: no replacements, lazy network fetches, alternate stores, hooks, shell interpolation, or content filters. Preserve safe typed errors and private redacted causes. Drain stderr concurrently without retaining unbounded raw payloads; do not print source bytes in diagnostics. On every ordinary success/error/drop path, close pipes and reap the owned child. Preserve the existing SIGINT listener, which uses `process::exit(1)` and bypasses destructors: test that OS pipe closure makes the real Git batch child exit promptly and that the CLI still emits one cancellation response. Do not promise parent-side reaping after the parent has exited or redesign the signal lifecycle in this optimization. An adversarial child ignoring EOF is outside this real-Git cancellation guarantee; ordinary error cleanup must still terminate and reap owned children.

`pin` still verifies the full closure before success, including existing refs. Lightweight `retain` validates the root and ref only; it must not construct `RetainedTree`. Full check/reindex audit all effective bases before any SQL publication. Ordinary refresh safely retains roots before SQL publication and verifies addressed data on demand. No persistent verified bit is introduced.

## Validation and acceptance

1. Profile all three workflows and identify the measured hot path before implementation. If profiles contradict the hypothesis, revise the proposal before editing product code.
2. Preserve the strong pin corruption, swapped-object, missing-object, ref-conflict, concurrent pin, GC/reopen, promisor and alternate-store tests for SHA-1/SHA-256. Test lightweight retention separately; existing refs cannot bypass full-audit damage detection.
3. Test binary framing, embedded LF/NUL bytes, short reads, oversized/malformed headers, wrong OID/type/length, premature EOF, stderr pressure, child failure and cleanup. Use independent hash literals or Git as the test oracle; never the new verifier itself.
4. Prove full-check/reindex audit precedes SQL writes and failure prevents publication. Prove routine refresh still retains roots before publication, skips exhaustive walks, and leaves unchanged revisions/bodies alone. Test scoped versus full check, no-change audits, newly introduced bases, post-pin unrelated corruption, addressed corruption, missing roots becoming available, concurrent publication, and inspect snapshot/hash consistency.
5. Mutation-check the new tests and restore the exact product bytes. Run `make check` and `make verify`; run the host storage/native runner if dependency/build changes require it. State unrun platforms explicitly.
6. Deterministic performance regression: routine reads never start source-closure verification, including for newly declared bases and after unrelated file edits. Full audit uses a constant process count per unique root as closure size grows, with one batch session and no subprocess per blob/tree. Inspect may batch-verify its new reading snapshot, but must not traverse unrelated author-source objects. Do not use wall-clock thresholds in ordinary tests.
7. Repeat matched before/after measurements and inspect stacks. Target at least a tenfold reduction from the observed dev-build latency, and aim for sub-second warmed local reads. These are measurement targets, not promised results; report remaining time in refresh, provenance, inspection capture and provider work separately. If the target is missed, profile the remaining cost rather than remove correctness checks.

## Alternatives rejected for this change

- Removing refresh: changes automatic-freshness and CLI contracts unnecessarily.
- Treating an existing ref as full verification: a ref proves reachability, not integrity. The accepted lightweight operation has its own explicit contract; strong pin/full audits retain repeated-corruption detection.
- Disabling hash recomputation: accepts a decodable object stored under the wrong OID.
- Keeping exhaustive source verification in every read: contradicts the user's accepted fast-read/full-check boundary.
- Verifying the entire object database: examines unrelated history beyond the declared snapshots and changes the scope of failures.
- Persistent process pools or daemon/watchers: add lifecycle and synchronization state without a measured need.

## Required contract updates

Amend X-1 and X-3, relevant mechanics/decisions, service trait documentation, CLI help and `skills/letopis/SKILL.md` to describe the distinction. Annotate the historical TK-26 checks as captured behavior at their original revision; do not overwrite old evidence or pretend those historical command-level corruption expectations still apply to every read. Add a dated decision linking this user choice and the new tests.

The [invariant matrix](../../zkit/letopis/checks/read-workflow-performance/invariants.md) is the acceptance checklist. It distinguishes current-code evidence from tests still required for the proposed implementation. A passing unchanged baseline is not proof that the new paths exist or satisfy the matrix.

## External protocol references

- [Git cat-file batch protocol](https://git-scm.com/docs/git-cat-file): ordered header, byte-counted body and trailing LF.
- [RustCrypto SHA-1 guidance](https://docs.rs/sha1/latest/sha1/): legacy SHA-1 interoperability and collision-detecting alternative.
- [sha1-checked 0.10.0 metadata and API](https://docs.rs/crate/sha1-checked/0.10.0): collision reporting, MSRV and docs-build limitation.
- [Git 2.50.1 mode normalization](https://github.com/git/git/blob/v2.50.1/object.h) and [tree decoding](https://github.com/git/git/blob/v2.50.1/tree-walk.c): compatibility oracle for the batch traversal parser.
