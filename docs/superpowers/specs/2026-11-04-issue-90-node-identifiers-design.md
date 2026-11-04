# Issue #90: simplify persisted node identifiers

Status: integer-endpoint revision complete. Tasks 1–4 passed independent specification and quality gates. Whole-branch review identified I1/M1/M2; one correction wave at `c1f2810` passed fresh host/Linux acceptance and a clean scoped rereview. Final code/evidence readiness is approved. The original report validates the schema-3 tree only; see the [integer-edge acceptance](../plans/2026-11-04-issue-90-integer-edges-report.md).

Issue: [#90](https://github.com/zdkadiev/letopis/issues/90).
Workspace: branch `issue-90`, based on freshly fetched `origin/main` at
`f0d5b8b733e7ab50bb7efb15d0d22daff438874a`. The separate `rafactor` branch is
preserved and is not a prerequisite or part of this task.

## Endpoint revision requested after PR #91

Accepted user decisions: both `edges.source_id` and `edges.target_id` are integer
foreign keys referencing `nodes(id)`; publication saves nodes first and edges
second in the same transaction. Missing targets remain visible to check/status,
including after target deletion. No declaration is silently discarded.

The concrete persistence and lifecycle design is specified below. The user has
approved the two-pass approach and the separately reviewed revised implementation
plan, then authorized Subagent-driven execution. The original report validates the original textual-edge
tree only, not this revision.

## Intent and scope

Remove duplicate textual identity from `nodes` while keeping one internal integer
key and one unique textual address. Existing element IDs, source addresses,
Markdown link semantics and CLI responses retain their meanings. This is a schema
and query simplification, not a new graph model or transaction framework.

The user approved rebuilding the local cache rather than automatic migration.
The full design was approved after clarification that the current textual fields
normally match, while the new integer ID and textual URI serve distinct purposes.
No product code or dependencies change before approval of the subsequent
implementation plan.

Design sources:

- [Architecture](../../zkit/letopis/architecture.md): synchronous hexagonal design;
  storage owns SQLite, services own application orchestration.
- [Mechanics MC-2/MC-3](../../zkit/letopis/mechanics.md): atomic publication,
  stable FTS joins, declared dangling links and hash-addressed vector reuse.
- [Active schema](../../zkit/letopis/mechanics/storage-schema.sql).
- [Decisions](../../zkit/letopis/decisions.md): D-91 through D-95 distinguish
  canonical Markdown, rebuildable cache, persistent vectors and element identity.
- [I-8](../../zkit/letopis/contracts/I-8-read-view.md): one pinned read view.
- [Development guide](../../development.md): explicit cache-only upgrade procedure.

## Original pre-implementation source baseline

`nodes.row_id` is an integer primary key with AUTOINCREMENT; `nodes.id` is unique
text; `nodes.uri` is required text without uniqueness. Source rows have an SQL
`id = uri` constraint. Knowledge rows have neither that equality constraint nor a
storage write/decode check enforcing it.

The production document publisher builds knowledge metadata with
`id = element.id` and `uri = element.id`. Source writes derive both stored strings
from one `SourceUri`. However, the public storage metadata type contains two
independently writable knowledge fields, and
`knowledge_and_source_optional_fields_survive_reopen` in
`tests/storage_cache.rs` deliberately persists `a.md#Section` as the ID with
`a.md` as the URI. Its behavior contradicts the documented metadata invariant.
The new implementation must resolve this explicitly, not assume all accepted old
inputs already satisfy equality.

These findings concern checked source and tests, not a scan or proof about any
user's live database. Existing caches can contain mismatches or duplicate URIs.
The approved rebuild strategy avoids mapping or silently merging such rows.

## Identity contract

The persisted representation becomes:

```sql
create table nodes(
 id integer primary key autoincrement,
 kind text not null check(kind in ('vertex','metavertex','source')),
 uri text not null unique,
 -- Remaining metadata columns and applicable constraints are unchanged.
 ...
);
```

This excerpt is illustrative; the executable schema contains complete DDL.
Remove the obsolete source `id = uri` check because `id` is now numeric.
Retain all independent kind, hash, line-range, snapshot and search-hash checks.

`nodes.id` is private database identity used for FTS and bounded graph handles.
`nodes.uri` is the unique address used for lookup, public string IDs, ordering and
external edge endpoints. Do not cast a numeric ID to text to satisfy an existing caller.
Knowledge addresses retain the complete path plus explicit anchor when present;
source addresses retain current normalization and opaque HTTP/HTTPS semantics.
Uniqueness is shared by all kinds, preserving today's single textual namespace
and existing knowledge/source collision rules.

Keep the current application-level `KnowledgeMetadata.id` and `uri` fields for
this focused change. On writes, reject unequal knowledge values with a typed
`InvalidRequest` before node mutation; a failed write retains the transaction's
existing rollback/poisoning contract. Reads construct both fields from the one
persisted URI. Document equality in the application-owned storage interface and
replace the contradictory optional-fields test with a valid round-trip case plus
a separate rejection/rollback regression. Removing public Rust fields is outside
this task; SQL deduplication must not silently reinterpret an unequal input.

## Queries, FTS and edges

Change the private SeaQuery node identifiers and every ordinary query so numeric
operations use `Nodes::Id` and textual operations use `Nodes::Uri`. Use bound
parameters; retain the existing specialized FTS5/vec1 SQL exceptions. Audit raw
SQL, row decoders, fixtures and examples as well as builder expressions.

UPSERT conflicts on `uri`, updates metadata and returns the existing integer `id`.
It must never use INSERT OR REPLACE or update the existing numeric key. Updating a
knowledge item, including a derived vertex/metavertex classification change,
preserves its FTS key. Deleting a node removes its FTS entry. AUTOINCREMENT stays;
keys are stable for an existing row, not promised across deletion or cache rebuild.
Internal `row_id` fields and `GraphNodeKey` may keep their current names: their
meaning remains a SQLite row identity, and renaming all Rust APIs adds no required
behavior. `vectors.db.vector_keys.row_id` and `vector_index.rowid` are separate keys
and must not be renamed or equated with a cache key.

Resolved edges use `source_id integer not null references nodes(id)` and
`target_id integer not null references nodes(id)`. Keep the composite primary key
`(source_id, target_id, type)`, self-edge prohibition and directional indexes.
Both references use the default restrictive deletion behavior; no cascade may
silently remove an incoming declaration. Decode external addresses by joining
both endpoints to `nodes.uri`; numeric keys never escape as public element IDs.

### Missing targets and lifecycle

Implementation choice: use a small `unresolved_edges` table for declarations
whose target does not exist. It has `source_id integer not null references
nodes(id)`, `target_uri text not null`, `type text not null`, and primary key
`(source_id, target_uri, type)`. Index target URI for promotion when it appears.
This table retains declaration identity and diagnostics; it contains no fake node
metadata, FTS document, embedding work or traversable edge. An unresolved target
cannot carry a foreign key to a nonexistent node. Source existence and existing
domain edge validation remain mandatory.

A logical declaration appears in exactly one table in every committed state.
`get_edges_by_source_id` returns the ordered logical union of resolved and
unresolved declarations, with both endpoints textual. Existing graph diagnostics
therefore continue to report the same `dangling_target` details without a second
error format or storing a permanent error string. Bounded neighbor queries join
only resolved integer edges, order/page by neighbor URI and exclude unresolved
links. No new public service interface or domain entity is needed.

Builder query choice: integer endpoint indexes no longer directly provide lexical
neighbor order. Preserve the TK-23 no-degree-sized-sort memory contract using
index streaming where possible or SQLite's bounded top-K `order by ... limit`
working set. Each query has a positive limit of at most the existing 32-record
page quantum and no OFFSET. Native query-plan/VM tests must prove bounded
retention (at most the requested LIMIT records plus constant working-record
overhead), rejecting unbounded sorters, automatic indexes and other degree-sized
materialization rather than merely suppressing the old blanket `TEMP B-TREE`
assertion.
The pinned bundled SQLite source has an explicit LIMIT+OFFSET bounded-sort path;
prove the actual emitted queries use it on host and Linux. If they do not, stop
and revise the query design rather than accepting unbounded materialization.
This preserves memory bounds, but a page may scan more adjacent edges than the
original text-key index, and repeated-page latency may grow with degree. Do not claim equivalent index-only performance or a
measured speedup. Test a high-degree node and all directional/filter/cursor cases.

- Inserting a declaration resolves its source by URI; absent sources and duplicate
  declarations fail and poison the write unit. Existing targets produce integer
  edges; absent targets produce unresolved declarations. The duplicate rule spans
  both representations, including a target inserted earlier in the transaction.
- Before deleting a node, check its outgoing declarations as before. If any remain,
  fail without publication. Otherwise demote incoming resolved edges into
  unresolved declarations using the target URI, then remove those resolved edges,
  its FTS entry and node. A missing node remains an idempotent no-op.
- At commit, promote every unresolved declaration whose target now exists into
  a resolved integer edge and remove the unresolved representation. This also
  resolves links owned by unchanged Markdown and supports existing low-level
  callers that insert an edge before its target in the same transaction.
- `delete_outgoing` removes both representations. Removing or editing the owner
  clears obsolete findings; restoring a target resolves surviving incoming
  declarations using its new numeric ID. Target deletion and recreation inside
  one publication must not leave stale integer references.
- Every insert, demotion, promotion, FTS mutation, file state and revision update
  belongs to the caller's existing cache transaction. Any failure rolls all of
  them back; a reader sees the old or new state, never an intermediate graph.

### Two-pass publication

The current refresher already uses one transaction for the whole delta, but
publishes each file's edges before later files and source nodes. Change that order:

1. For changed/removed valid projections, clear old outgoing declarations and
   perform node removals, node UPSERTs, FTS changes and file-state changes using
   the current per-file preparation. Invalid Markdown retains its last-valid
   projection and error behavior. Reconcile derived source nodes before pass 2.
2. Insert changed owners' current declarations using the final effective node
   set. Reconcile unresolved declarations, then increment revision and commit.

Keep one-file search text lifetime and the existing effective metadata graph;
do not retain all captured Markdown or duplicate the graph to implement two
passes. Track changed owners only, reuse their edges already in the effective
graph, and do not delete/reinsert every unrelated edge on an incremental refresh.
Existing unchanged incoming declarations are updated only when resolution changes.
An `external` target with a source row but no source bytes remains a source node
with null hash, following existing source-availability rules; it is not a missing
knowledge target. Existing arbitrary relation types and includes rules remain.

Preserve lexical address ordering for search tie-breaking, graph neighbor cursors,
embedding work selection and exported node lists. Numeric key ordering is not an
equivalent replacement. Search filters, fusion, pagination bounds, revision
semantics and pinned cache/vector reads remain unchanged.

## Cache compatibility and upgrade

Advance the cache schema version to 4; keep vector schema version 1.
The previously reviewed schema-3 textual-edge layout is also incompatible. Exact
schema recognition continues to reject old or malformed cache layouts before
opening or modifying persistent vector storage. Normal open, status and reindex
must not silently migrate, clear, repair or reinterpret an incompatible cache.
An incompatible database returns the existing safe storage-schema diagnostic.

Here “intact” guarantees persistent cache main/WAL bytes and file identities,
logical committed state, vectors, configuration and lock files. Direct open and
reindex recognize the exact schema through a read-only connection before any
writable cache open; status remains read-only. This covers closed DELETE caches
and committed uncheckpointed WAL. Fresh/empty initialization and writable
bootstrap's schema revalidation under concurrent initialization remain supported.

All three entry points may create an empty cache WAL and its SHM reader index;
an existing SHM may update reader bookkeeping while retaining its identity and
safe lifetime. SHM bytes are not immutable. Readers observe committed live WAL,
never use immutable reads or unlink sidecars to hide these effects. Vector files
are never created or touched on cache rejection. This is not a zero-filesystem-
effects guarantee.

Document the existing explicit, quiescent upgrade procedure for this version:

1. Stop foreground commands and detached drain; close all database handles.
2. Back up the cache if old derived projections are worth retaining. Remove only
   the cache database and its cache-specific WAL/SHM sidecars after quiescence.
3. Keep vectors and their sidecars, configuration, lock-file identities, Markdown
   and Git intact. Do not unlink lock files while another process may hold them.
4. Run the supported `reindex` command to republish the current Markdown corpus
   into schema 4. Subsequent ordinary commands use that cache.

This is an operator procedure, not permission to delete a live user cache during
development or tests. Upgrade tests work only on temporary fixtures. The rebuild
does not recover a last-valid projection for currently invalid or missing Markdown;
such information may exist only in the old cache. It follows current reindex
validation behavior and never claims old integer IDs or revisions are preserved.
An invalid, inaccessible or damaged cache is not automatically accepted as empty.

Old mismatched IDs/URIs and duplicate URIs are not migrated or deduplicated: the
legacy database is rejected untouched, and canonical Markdown establishes the new
projection. Do not modify captured legacy compatibility fixtures to make them look
like schema 4. Use a separate frozen schema-2 fixture for upgrade rejection and
retain historical schema-1/2/3 evidence against its original revision. Freeze the
current schema-3 DDL separately before editing it; do not reuse the schema-2
fixture as the schema-3 oracle.

## Vector compatibility

Vectors, configuration identity, failures, attempt counters and query diagnostics
remain in their unchanged database. Cache-to-vector membership joins continue
through `search_hash`, never integer node IDs. A cache-only rebuild therefore
reuses vectors for identical current text without reinference, even when cache
integer IDs differ. The schema transition itself performs no vector reset or
prune. Ordinary later configuration changes or drain pruning keep their existing
rules; preserving the file does not promise permanent retention of orphan vectors.

## Architecture and non-goals

Implement on the storage interfaces and transaction model actually present in
`main`. Do not import the separate storage-transaction refactor, add database
owners, introduce an ORM, runtime, pool, abstraction or dependency for this task.
Domain values stay pure; public CLI identity remains textual. Keep the existing
storage/service/composition boundaries. No provider calls, paid sessions or
performance claims are needed to demonstrate schema correctness.

## Acceptance evidence required during implementation

1. Inventory every production node write and decoder. Show valid whole-file,
   anchored, metavertex, local-source and remote-source cases; verify rejection of
   unequal knowledge metadata with no partial publication. Distinguish supported
   writer invariants from unenforced properties of old caches.
2. Assert actual SQLite column types, unique URI enforcement, correct foreign key
   target and stable UPSERT numeric IDs using an independent SQLite oracle. Verify
   rollback for duplicate/conflicting writes rather than checking only query text.
3. Exercise FTS insert/update/delete, address-based tie ordering and filters,
   graph paging in both directions, includes, dangling-link disappearance/return,
   source pruning, inspect and check/status using public interfaces or the CLI.
4. Reject frozen old schema 2 and malformed layouts without changing retained
   cache content or vector content/identity; preserve schema-1 coverage where it
   remains relevant. Include old knowledge mismatches and duplicate URI cases.
   Test observation-only status rejection and the supported rebuild procedure.
5. Populate vectors and attempt/failure state under the old cache; rebuild only
   the cache; verify exact vector bytes, configuration and failure metadata survive
   and current text reuses vectors with no document embedding request. Deliberately
   change cache key allocation so this cannot pass by accidental key equality.
6. Keep executable and design schemas in parity. Update active contracts,
   development upgrade instructions and applicable source-linked knowledge;
   preserve historical schemas, harnesses and captured evidence.
7. Run `make verify` and the relevant host/Linux storage acceptance runner with
   new evidence directories; report exact results and unavailable platform limits.
   Mutation-check new behavioral regressions, restore each mutation and verify the
   final tree. An independent final review examines identity mapping, FTS keys,
   foreign keys, vector isolation, schema compatibility and module boundaries.

## Revision acceptance additions

- Independent SQLite introspection verifies both resolved endpoint columns are
  integer, both foreign keys reference `nodes.id`, and nonexistent numeric
  endpoints are rejected. Verify unresolved source foreign keys and no phantom
  nodes. `pragma foreign_key_check` returns no rows after every lifecycle case.
- Two files introduced together in reverse target order, anchored targets and
  derived local/remote source nodes resolve before publication. Test initial
  reindex and incremental refresh with an unchanged link owner.
- Missing, appearing, deleted and recreated targets preserve literal diagnostics,
  declared-edge identity and traversal exclusion/visibility; duplicate insertion
  and deleting a source with unresolved outgoing links still poison/roll back.
- Faults after pass 1, demotion and promotion restore node/FTS/edges/unresolved
  declarations/files/revision together. Manual mutations prove the new tests fail.
- Frozen schema 3 is rejected untouched, alongside retained schema-1/2 coverage.
  Cache-only rebuild preserves exact vector bytes and retry/config/query-error
  state, with deliberately different cache/vector numeric allocations.
- Repeat host/Linux native storage acceptance with new evidence directories,
  update active docs/schema parity, and obtain independent review. Keep the
  original report/evidence labeled as original implementation, never overwrite it.

## Approval boundary

The user approved local cache rebuilding, explicitly required the branch to
start from `main`, and approved this specification, the separate implementation
plan and Subagent-driven execution. All four tasks, host/Linux native acceptance
and independent task/whole-branch reviews are complete with zero review findings.
The whole-branch review covers main `f0d5b8b..ff84a37`; see the
[acceptance report](../plans/2026-11-04-issue-90-node-identifiers-report.md).
PR #91 contains that original implementation. The user subsequently approved
integer edge references and two-pass publication. The user approved the revised specification and implementation plan, then
explicitly authorized Subagent-driven implementation. Prior review results do not cover this revision;
no merge or issue closure is implied.

For this integer-endpoint revision, Tasks 1–4 have independent specification and
quality approval. The whole-branch review of `f0d5b8b..5872c9e` returned **With
fixes** for I1/M1/M2. One consolidated correction in `c1f2810` passed fresh
host/Linux native acceptance and exact input reconciliation. The scoped rereview
of `5872c9e..c1f2810` closed all three findings with no new breakage or out-of-scope
observations and approved final code/evidence readiness. Historical findings and
receipts retain their original scope; no merge, push, PR update or issue closure
is implied by this approval.
