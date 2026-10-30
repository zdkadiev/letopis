# Product Git and provenance optimization — issue #67

Date: 2026-10-30. Status: approved by the user on 2026-10-30;
implementation has not started. Implementation-plan preparation is authorized.

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
not all their payloads. For full-audit blob verification, preserve the existing
fixed-size streaming buffers: working payload memory must not grow with blob
size, including transient allocation of one complete blob by the library. This
is the specification author's explicit preservation choice, not approval of an
unmeasured library behavior. Existing addressed-blob APIs may return one blob's
bytes; that separate contract does not permit buffering blobs during closure
audits. Loose/packed access, including delta reconstruction, is an early
compatibility gate. If bounded streaming requires an operation-owned temporary
spool, the plan must document its disk/cleanup/cancellation behavior; do not add
a persistent object store or reject previously valid large blobs to avoid it.
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

Independent builder review found one ambiguity: whether a full audit could
temporarily allocate an entire blob. Resolved above by explicitly preserving
fixed-buffer audit payload memory; the addressed-blob API remains separate.

This specification does not mark #67 implemented, close #65, alter #65 evidence,
authorize paid runs, or merge/push unrelated local documentation changes.
