# Product Git and provenance optimization — issue #67

Date: 2026-10-30. Status: approved by the user on 2026-10-30;
implementation and the measured campaign are complete through Task 8. The final
independent review found a sparse-index regression; its focused fix and evidence
refresh are recorded in the current report. Scoped independent re-review passed: both findings are addressed, with no new findings.

Task: [product optimization issue #67](https://github.com/zaurkadiev/letopis/issues/67).
Base: `32ca0af282a1321d0137b4aba115c6fddc086259`, containing the diagnostic
[performance pull request](https://github.com/zaurkadiev/letopis/pull/66).

## Intent and accepted direction

Reduce measured product latency without weakening source provenance, integrity,
snapshot capture, retention or concurrent-publication guarantees. The user requires
all product Git calls to run inside Letopis through a Rust library, while retaining
the existing Git snapshot storage. On 2026-10-30 the user selected the proposed
`gix` approach and explicitly included previous batch/provenance findings and a
clear measured before/after table. Use Superpowers: approve this specification,
then prepare and separately approve an implementation plan before product changes.

The recent diagnostic work measured bottlenecks; it did not implement the batch
verifier. Batch verification and lightweight routine retention already shipped in
the earlier [read-workflow change](2026-10-29-read-workflow-performance-design.md).
This task reuses that implementation's rules and removes repeated work rather
than introducing a second verifier.

## Approaches

1. **Selected: gix behind the existing application-owned interfaces.** Replace
   discovery, object access, strict validation, listing and refs in process;
   reuse command-scoped repository state and the existing independent hashing
   and closure traversal. Deduplicate provenance separately at the service owner.
   Preserves storage and architectural boundaries; requires explicit parity tests.
2. **Alternative: git2/libgit2 bindings.** Also avoids subprocess transport, but
   introduces the C library boundary and a different SHA-256 compatibility surface.
   It does not avoid the same integrity/ref/listing parity work. Not selected.
3. **Rejected: keep a persistent/batched Git child and only deduplicate.** Smaller
   transport change, but fails the user's zero-product-Git-subprocess requirement.

The current assessment used [gix 0.90.0](https://docs.rs/gix/0.90.0/gix/).
Implementation must pin a tested version and the smallest required feature set,
including SHA-256 support, against Rust 1.95.0. Do not install dependencies during
specification. Feature/API compatibility is a first implementation gate, not an
assumed result. If gix cannot meet a required guarantee, report the concrete gap;
do not silently keep a CLI fallback, change libraries, or relax the guarantee.

## Scope and boundaries

- Cover every product Git path: repository discovery and linked worktrees,
  object format, candidate enumeration, raw blob/tree reads and writes, structural
  validation/full-closure pinning, refs/retention and capability checks. No hidden
  helper, filter, shell, lazy fetch or network process may substitute for Git.
- Keep existing snapshots, raw bytes, full SHA-1/SHA-256 OIDs and service refs
  readable without rewriting objects or migrating storage. User Git operations,
  test fixture setup and independent verification may still use the Git executable.
- Keep synchronous I/O, per-invocation CLI processes and existing locking. No
  async runtime, daemon, parallel drain, new persistence engine or new cache table.
- Keep `gix` types private to `src/git/`. A concrete `GitRepository` in
  `src/git/repository.rs` implements the existing application-owned snapshot,
  verification and corpus-listing traits. `src/lib.rs` composes dependencies.
  Domain values stay infrastructure-free; provenance orchestration stays in
  services. Private object/ref/validation helpers remain responsibility-named.
- Do not propagate new generic parameters, introduce forwarding services, or
  mechanically refactor unrelated services. Follow [AGENTS.md](../../../AGENTS.md)
  and [program structure](../../zkit/letopis/architecture.md#program-structure).
- Update D-96, X-3, D-105 and affected architecture, mechanics, deployment/developer
  guidance and agent instructions to replace the CLI-specific implementation
  statements. Preserve their historical decisions and evidence with dated updates.

## Mechanisms and invariants

### Repository and object operations

Reuse repository state within one invocation; do not rediscover root/config for
each object. Restrict configuration/features to local raw object operations:
no filters, replacement objects, alternates, promisor fetching or executable
helpers. Preserve the current operation-entry alternates check, including a file
added after repository opening; cache its path, not a permanent policy success.
Preserve worktree discovery, path containment and candidate behavior equivalent
to `ls-files --cached --others --exclude-standard`, including tracked ignored
files, untracked ignore rules, deduplication, invalid paths and private directories.

Object availability alone does not establish integrity. Verify exact type,
repository hash width and Git header/body identity with collision-detecting SHA-1
or SHA-256. Keep existing `ObjectHasher` logic where suitable; validate compressed,
loose and packed data instead of trusting an ODB lookup or cached decoder result.
Do not peel commits/tags into source trees or normalize captured file bytes.

Retain the existing closure walk's visited/type-conflict rules and gitlink behavior.
Replace subprocess reads with in-process access, not a second closure algorithm.
Replace subprocess structural checks too: decoding tree entries alone does not
establish the current strict acceptance/rejection behavior. Independent Git fixture
oracles must pin malformed-tree, mode, name and ordering behavior before replacing
`TreeValidator`. Do not silently tighten or loosen accepted historical trees.

Test-oracle version clarified during Task 7: strict tree parity is against the
captured host Git 2.50.1 `hash-object -t tree --no-filters` operation. Debian
Git 2.39.5 performs weaker parse-only checks and accepts malformed cases rejected
by that approved reference. Pin upstream Git 2.50.1 as the Linux fixture/oracle
build tool, with a recorded source checksum/version; do not relax the product
validator or skip differential cases. This adds no product Git executable
dependency and does not promise every historical version's malformed-tree
acceptance. Keep the exact hash-object operation, not a full `git fsck --strict`
substitute. Versioned primary sources: [2.50.1](https://github.com/git/git/blob/v2.50.1/object-file.c#L1089-L1095)
and [2.39.5](https://github.com/git/git/blob/v2.39.5/object-file.c#L2165-L2219).

Preserve the distinction: root verification checks its own type/bytes; routine
reads verify addressed objects; `retain` verifies the root and exact direct ref;
`pin` verifies the full local closure even when its retention ref already exists.
Whole-corpus check/reindex audit author bases before SQL publication under
`index.lock`; inspect strongly pins its new reading snapshot. Successful root
verification is never silently promoted to a strong `RetainedTree` capability.

### Retention and durability

Keep exact direct refs in the existing namespace. Reject symbolic refs, including
dangling ones, and conflicting targets. Matching direct refs are idempotent;
create-if-absent races succeed only after confirming that exact direct target.
Preserve concurrent lock failures, safe error classification and required object/
ref durability. Prove the selected library's write/flush/lock behavior against the
current contract; a transaction API alone does not establish multi-ref atomicity
or power-loss safety. Do not claim a new crash or NFR guarantee without evidence.

Implementation choice following contract review, 2026-10-30: Letopis's private
retention refs do not create or append reflogs, even when Git configuration would
make CLI update-ref do so. Existing private/user logs and user refs stay untouched.
X-3 and the application traits require durable direct refs and snapshot retention,
not automatic reflog creation or OS-derived committer identities. Do not implement
a Git identity/DNS-emulation subsystem or invent a dummy identity for internal
retention. This intentionally differs from an incidental CLI side effect and must
be covered/documented; it does not relax ref durability or historical object access.

### Provenance deduplication

Own reuse at the narrow operation that already owns the observations; do not add a
global cache or a technology cache inside domain data. Group equivalent root and
historical-source work by exact tree OID, path and range. Reuse verified immutable
bytes/hashes only within an explicit operation, so repeated knowledge bindings
do not repeat equivalent historical reads/hashes. Repeated source-revision filters
use the same owner and error rules where their lifetimes coincide.

Current source files are mutable: no command-long path-only hash cache that hides
edits between observations. Group reads only where an existing capture/observation
boundary defines the same input; otherwise retain a fresh observation and existing
stat/hash/freshness checks. Keep bounded metadata/hash reuse, release file contents
after use, and avoid retaining every source blob or the full graph. The implementer
chooses the smallest concrete helper/lifetime after tracing actual service calls;
do not broaden trait surfaces merely to share a cache. Tests must demonstrate an
edit between separate observation/capture boundaries remains detectable.
Unavailable basis classifications and infrastructure failures are recomputed,
not cached; a missing or invalid author root may become available during the
operation. Revision-comparison reuse also skips `BasisUnavailable` outcomes.

Implementation boundary clarified during Task 6: preflight and check/status
already hold the metadata needed for bounded multi-binding observation batches.
Search interleaves candidate-edge reads, revision checks, live knowledge reads
and provenance; traversal interleaves metadata, neighbor pages, provenance and
live reads. Keep those existing demand/error and capture orders. Their singleton
observations retain fresh state; this change does not provide cross-binding
historical reuse there. Search still reuses equivalent revision comparisons within
its eligibility operation. Do not add a cache-session interface or eager graph
loads merely to extend reuse beyond an existing observation boundary.

### Inspect capture

Reuse verified blobs and shared sources within capture where correctness permits,
removing redundant per-entry validation while retaining final closure validation.
Every public tree-assembly request still validates untrusted supplied identities;
do not treat arbitrary caller OIDs as already verified. An internal optimization
must make the trusted boundary explicit and preserve corruption detection.

Keep complete marked-corpus membership, all declared local source files, exact
captured bytes, paths/ranges/hashes, all-request unknown-ID failure and retention.
For a marked source captured earlier, use its immutable captured blob rather than
rereading changed live bytes. Preserve plain-file observation comparisons and
unavailable/remote source semantics. No requested-ID-only partial snapshot.

### Resource behavior

Do not introduce a whole-object-database or whole-source-content cache. Reuse bounded
buffers; any enabled library cache has an explicit byte cap and command lifetime.
Full audits retain visited identities/metadata proportional to reached objects,
not all their payloads. **Approved revision, 2026-10-30:** the user permits the
current object's in-process decoding buffers, including delta bases/results and
instructions. Audit working memory may therefore scale with that object/delta
chain; a fixed 64 KiB total payload bound is no longer required. Keep independent
hashing in bounded chunks and release payloads before advancing through the
closure. Keep object/delta caches disabled initially; do not retain every blob
or a giant reusable capacity indefinitely. Measure actual allocation/RSS and
report the per-object tradeoff rather than claiming constant audit memory.
Existing addressed-blob APIs may return one blob's bytes. Loose/packed/delta
correctness and resource behavior remain an early compatibility gate. No new
pack/delta engine or persistent spool/store is needed or authorized by this
revision; do not reject previously valid large blobs just to enforce a new cap.
Cache limits and oversized-object behavior are implementation choices to record
in the plan and measure, not new arbitrary rejection limits on existing inputs.

## Before/after measurement contract

Preserve #65 artifacts against their captured revision. Its observations are
historical context; reproduce a fresh ordinary baseline on this task's base
revision and compare the optimized revision on the same host, corpus, toolchain,
build profile, configuration and provider-free conditions. Record binary hashes,
revision identities, fixture generation/seed, exact commands and environment.

Reuse the diagnostic harness where applicable. Measurements must execute immutable
old/new binaries, verify exit status and independently validate outputs, errors,
flags, source ranges and addressed bytes. Compare semantic output when fresh
fixture paths/revision counters differ legitimately; keep fixtures equivalent.
Use separate reproducible corpus copies when operations mutate objects or cache;
declare warmup, OS-cache limits and initialization. Interleave matched samples to
reduce order bias. Keep timing unwrapped; collect instrumentation and profiles in
separate runs. Python Git-counter durations and inclusive sampled CPU are not
subtractable wall-time fractions.

Use at least 10 successful samples per feasible initialized check/inspect/traverse
scenario. Report median, min/max or percentile/spread, sample counts, timeouts and
budgets, plus ratio and percent reduction. Single observations and censored runs
remain explicitly marked. Provisional target: at least 50% lower 60-element check
median, without loss of correctness; a miss is reported and explained honestly.
No minimum overall speedup or formal project latency/RAM verdict is assumed.

The final report must include this populated table; `not assessed` is valid where
the measured evidence cannot establish a result, never an invented number:

| Scenario | Before median / n | After median / n | Reduction / speedup | Git subprocesses before → after | Peak memory before → after / scope | Correctness / limits |
| --- | --- | --- | --- | --- | --- | --- |
| Initialized check, 60 elements with external links | pending | pending | pending | pending → 0 | pending | pending |
| Initialized inspect, 1k | pending | pending | pending | pending → 0 | pending | pending |
| Bounded traverse, 1k | pending | pending | pending | pending → 0 | pending | pending |
| FTS search, 1k (control) | pending | pending | pending | pending → 0 | pending | pending |
| First publication and full audit, 1k | pending | pending | pending | pending → 0 | pending | explicit timeout budget |
| Reindex with concurrent reader, 60 | pending | pending | pending | pending → 0 | pending | verify valid overlap |

Historical context: #65 check 60 was 11.918 s (n=3), with 1,183 Git calls
in a separate instrumented run; matched ordinary inspect/traverse 1k were
4.183/1.909 s (n=3), with 378/180 Git calls in separate counter runs. These are
not the fresh baseline and cannot be substituted for it without a documented
comparability justification. First publication 1k took 202.193 s once in a 600 s
exception; several 1k audits and 10k/100k initialization hit 120 s budgets.

Reattempt 1k publication/audit/timeout scenarios with explicit bounded budgets;
larger scales only when feasible. Measure reindex reader blocking separately from
SQLite contention. Report direct CLI peak RSS and any descendant/aggregate scope
separately. Use samply for before/after bottleneck profiles; profile overhead and
off-CPU limitations remain separate from ordinary latency. Counters should expose
object reads/bytes hashed/source observations and lock timing where available;
keep performance instrumentation out of the public product command contract.
No paid/model sessions or real-model latency claims are required.

## Verification and acceptance

1. Regression fixtures independently establish SHA-1/SHA-256, loose/packed,
   malformed/corrupt/collision objects, no-peeling, historical readability,
   strong-vs-weak retention, direct/symbolic refs and concurrent CAS/locks.
2. Corpus/worktree/ignore/path tests preserve exact eligibility and repository
   policy. No external filters, helpers, alternates, replacements or lazy fetch.
3. Freshness tests exercise shared sources/ranges, edits between observations,
   inspect marked-source capture reuse, missing sources and all-request failure.
   Independently counted shared work verifies actual deduplication, not merely
   matching outputs. Mutation-check new behavior tests and restore mutations.
4. A subprocess-denial regression runs product bootstrap, init/config/status,
   check, refresh/reindex, search, inspect, traverse, drain and relevant error paths
   after independent fixture creation. Cover product invocations with Git absent
   or poisoned and audit process-spawning sites; a PATH-only assertion cannot
   prove absence of absolute executable calls. No permanent CLI fallback.
5. Run `make verify` on the actual changed tree and relevant host/Linux native/
   build checks affected by dependency/linkage changes. Report platform limits;
   root tests alone do not establish standalone distribution or Linux parity.
6. Review physical module layout, visibility, imports, traits, lib.rs constructors,
   safe typed errors, function sizes and Given/When/Then tests against AGENTS.md.
7. Publish before/after receipts/profiles, a complete table and remaining
   bottlenecks, tied to exact source/build identities. Store fresh evidence under
   an issue-67 directory; do not overwrite issue-65 captures. Report missing gates
   honestly instead of treating administrative issue closure as technical proof.

## Builder choices and remaining gates

Selected direction and invariants above are fixed. The implementation plan chooses
precise gix feature flags, private helper layout, bounded-cache values and the
smallest provenance grouping mechanism. It must map compatibility fixtures and
measurement budgets to concrete steps/files before code starts. No new user-facing
options, storage formats or public service abstractions are needed by this design.
Unknown library streaming/durability/strict-tree capabilities must be checked early;
an unresolvable guarantee is a reported blocker rather than a hidden compromise.

Independent builder review originally resolved audit-memory ambiguity in favor
of fixed buffers. Subsequent pinned-source inspection found ordinary gix packed
decoding cannot meet that condition. The user explicitly superseded that choice
on 2026-10-30 by permitting operation-local per-object/delta buffers while keeping
caches bounded and all integrity/durability requirements. See the
[compatibility findings](../../zkit/letopis/checks/issue-67/compatibility.md).

This specification does not mark #67 implemented, close #65, alter #65 evidence,
authorize paid runs, or merge/push unrelated local documentation changes.
