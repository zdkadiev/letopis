## Storage layout and schemas {#schemas}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Markdown and source files remain authoritative. `.letopis/cache.db` is the
rebuildable graph and search projection; `.letopis/vectors.db` holds vector state
separately so rebuilding the cache does not inherently discard those vectors.

The cache schema contains a publication revision, `nodes`, `edges`, the contentless
FTS5 `text_index` and per-file observations. A node has a text identity and an
integer row key. Resolved edges store source/target node keys and type, with foreign
keys on both endpoints. Missing knowledge targets are recorded separately in
`unresolved_edges` by `target_uri` for diagnostics until they can be resolved.
The FTS row is associated with the node row key. Invalid-file state belongs to
the file record.

The vector database tracks configuration identity, `search_hash` to vector keys,
vec1 cosine-search data, failed attempts and the latest query error. Its index
uses `search_hash` to join vectors to current knowledge nodes; identical search
text can therefore share a vector. The schema is independent of the cache schema.

[Cache schema](../../src/storage/cache.sql "letopis:type=external;path=src/storage/cache.sql")

[Vector schema](../../src/storage/vectors.sql "letopis:type=external;path=src/storage/vectors.sql")

[SQLite connections](../../src/storage/connection.rs "letopis:type=external;path=src/storage/connection.rs")

[Read-view and vector ranking](search-pipeline.md#request-bounds "letopis:type=related_to;path=letopis/architecture/search-pipeline.md#request-bounds")

## Transaction scopes and publication {#scopes}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

`CacheStorage` owns the cache writer and combined read connection; `VectorStorage`
owns the independent vector writer. Application-owned `CacheDatabase` and
`VectorDatabase` expose zero-argument closure scopes. Services share the same
`Rc` owner when nested calls must join: they use their injected fields instead
of passing transaction handles. Only the outer scope commits or rolls back.
A caught nested error, panic or failed write prevents publication. A read inside
a write sees its uncommitted changes; a write inside a read is rejected and
makes the outer scope fail, whether the caller attempts a nested write scope or
a direct write method and catches its error. A rejected read-only publication
revision accessor does not poison the read scope. Failed cleanup disables reuse
if SQLite stays active.

A cache read pins its revision and attached vector identity before the callback.
Publication advances revision once at the outer commit; the pending revision is
provisional. Vector writes verify expected identity and use a bounded busy wait.
Query diagnostics use the separate vector writer while search retains its view;
service-owned write scopes end before provider HTTP. Different owners and the
two databases do not join into a shared transaction.

Storage also validates use-case entry requirements without opening a transaction.
An incompatible inherited scope becomes rollback-only. Query inference permits a
cache read with an inactive vector owner; drain requires both owners inactive so
work selection can observe each completed vector publication.

[Cache scopes](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Transaction lifecycle](../../src/storage/transaction.rs "letopis:type=external;path=src/storage/transaction.rs")

[Vector scopes](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

[Direct write rejection tests](../../tests/storage_read_scope_failures.rs "letopis:type=external;path=tests/storage_read_scope_failures.rs")

[Application interfaces](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

## Status observation and file checks {#status-observation}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Status uses an observation-only CacheStorage. Absent storage needs no connection,
creates no durable file and skips the callback; an unpublished cache also skips
observation. The observed owner cannot write. `LocalFileSystem` provides generic
file/directory identity, no-follow and alias checks. Composition selects database
filenames and brackets opening/attaching with those checks; SQLite connections,
schemas and attachments remain in storage. Filesystem has no database-role component.
Consumers needing only bytes can use `file_get_bytes_by_path`: it retains capture's
identity, content and ancestry checks without sampling the clock or converting
filesystem timestamps into index metadata.

Cache node, edge, file, FTS and neighbor queries live with `CacheStorage` in
`storage/cache.rs`. Vector, embedding work, pruning and status-supporting SQL
observations live in `storage/vector.rs`; unit tests stay inline. `StatusService`
owns configuration-mode policy, identity interpretation and backlog/failed arithmetic.

[Generic file, path and identity checks](../../src/filesystem/local.rs "letopis:type=external;path=src/filesystem/local.rs")

[Composition](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Status policy](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

## Exact vec1 execution modes {#vec1-modes}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Integrated into the working tree on 2026-11-07, new and reset vector indexes use
`flat/cos`. `none` is exact cosine search through a SQL scan/sort of raw vectors;
`flat/cos` is also exact exhaustive search, using packed original f32 vectors,
the same cosine function and a compact result heap. It has no coarse buckets
or quantized codes. Actual-root host verification passed; the index-only
experiment below established no whole-CLI wall-time improvement. The later
bounded SQL experiment is recorded at the end of this concept.

Flat moves the authoritative full-vector payload into native index blocks;
base shadow rows become bucket references. Read vector bytes through the virtual
table. Do not drop/recreate this index as if it were a redundant secondary index.
The existing `search_hash` keys, rowids, dimensions and embedding identity must
survive a physical-mode change, together with attempts and query diagnostics.

[Native vec1 implementation](../../vendor/vec1/vec1.c "letopis:type=external;path=vendor/vec1/vec1.c")

[Schema recognition](../../src/storage/schema.rs "letopis:type=external;path=src/storage/schema.rs")

[Guarded physical index preparation](../../src/storage/schema.rs "letopis:type=external;path=src/storage/schema.rs")

## vec1 upgrade and verification {#vec1-upgrade}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Changing new-schema SQL alone does not upgrade an existing populated database.
Storage bootstrap now prepares recognized none/cos under a bounded immediate write
transaction, rechecking after acquiring the writer before native rebuild. The
storage-local preparation in `schema.rs` classifies opaque persisted model bytes against none/cos and
flat/cos models generated by the pinned native factory; it does not trust a decoded
header or `vec1cat` display that can conceal an unknown distance tag. Before legacy
conversion it rejects missing, wrongly typed, nonpositive or overflowing native MODEL
versions, mismatched vector lengths and nonfinite f32 components. Unknown models
remain errors. Already-flat, read-only and query-only compatible handles avoid a
migration writer; status remains observational. New/reset defaults are flat/cos;
vector schema version remains 1 and embedding identity is unchanged. Migration,
readback, rollback/retry, competing-opener and separate pinned-reader tests are
present.

[Actual-root verification executions](../../docs/zkit/letopis/checks/issue-65/vector-index/verification.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/verification.json")

[Host storage receipt](../../docs/zkit/letopis/checks/issue-65/vector-index/host-storage/manifest.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/host-storage/manifest.json")

[First checked product upgrade](../../docs/zkit/letopis/checks/issue-65/vector-index/first-upgrade-init.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/first-upgrade-init.json")

## vec1 coverage and probing limits {#vec1-coverage}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Ten targeted scenarios and six caught behavioral mutants accompany the
actual-root host storage runner PASS across all 16 commands. Its nested
`make verify` passed 2,443 test executions with zero failures and 22 ignored,
including profile/example repeats; these are executions, not unique tests.

Trained vec1 models can use coarse buckets and quantized codes. Full `K` controls
result capacity, not bucket coverage: default probing can omit candidates even
when `K` equals corpus size. `nprobe:1` means one bucket, not all buckets; complete
coverage requires the explicit bucket count and no early slack stop. Quantized
metrics can change rank even with complete coverage. Full eligible-channel rank
fusion and first-demand candidate error/freshness guarantees cannot be replaced
by ANN or winner-only observation without a separate accepted contract change.

## vec1 measurements {#vec1-measurements}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

A bounded native SQL experiment on copied 100,000 x 768 vector data measured
full-K median CPU 2.145223 s in none/cos versus 0.107106 s in flat/cos (20.03x).
Three paired repetitions at K=5/100/1000/100000 matched the full-result prefixes
in IDs, order and 17-digit distances; ordered hashing also matched all vector
BLOB bytes after rebuild. These are native SQLite/vec1 results from a stored
embedding query, excluding CLI refresh, provenance, filters, fusion and inference;
this native diagnostic used SQLite CLI 3.54.1 versus product SQLite 3.53.2.
It does not establish a whole-command speedup, fresh singleton-drain layout or
write-path cost.

Actual-root product init on the populated none/cos copy, including checked upgrade,
took 3.435155 s and preserved all 100,000 vectors. Three paired full hybrid CLI
searches per mode on the synthetic 100k corpus with real stored provider vectors
measured median wall time 65.592816 s before versus 65.829875 s after: no whole-wall
improvement was established. All six outputs were byte-identical with the same five
IDs. Maximum observed CLI peak RSS was 187.9375 versus 158.921875 MiB, excluding
the Ollama model daemon; this is not a complete memory-budget result.

A separate genuine Samply capture contained 79,244 usable stacks and 70,303,137 us
observed CPU delta. The native vector-query owner accounted for 0.9954% inclusive
observed CPU and candidate eligibility for 96.7047%. These scopes overlap and must
not be added; sampled CPU shares are not elapsed-wall-time attribution. The full
CLI evidence, not the isolated native speedup, controls the end-to-end conclusion.

[Native experiment and corrected controls](../../docs/zkit/letopis/checks/issue-65/vector-index/native-probe/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/native-probe/report.md")

[Native paired measurements](../../docs/zkit/letopis/checks/issue-65/vector-index/native-probe/summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/native-probe/summary.json")

[Whole-CLI paired comparison](../../docs/zkit/letopis/checks/issue-65/vector-index/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/comparison-summary.json")

[Samply capture validation](../../docs/zkit/letopis/checks/issue-65/vector-index/profile/validation.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/profile/validation.json")

[Inclusive observed CPU stacks](../../docs/zkit/letopis/checks/issue-65/vector-index/profile/stacks.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/profile/stacks.json")

[Profile scope and process receipt](../../docs/zkit/letopis/checks/issue-65/vector-index/profile/receipt.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/profile/receipt.json")

## vec1 write layout and corruption {#vec1-corruption}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Source inspection shows a singleton flat insert can replace a whole small packed
block at commit; the default reuse threshold is 16 KiB, so it does not rewrite an
existing approximately 1 MiB rebuilt block on every append. Incremental BLOB writes
serve delete tombstones, not insertion appends. An index grown by singleton commits
can have a different layout from a bulk rebuild: the native 20.03x query result
measured the rebuilt 100k layout. Fresh-drain throughput, SQLite page/WAL write cost
and performance relative to the earlier approximately 142-minute model drain remain
unmeasured; the read result does not establish those write-path properties.

Corrected raw-typed corruption controls found a 3-byte vector rejects rebuilding
and leaves none mode intact; injected NaN/infinity components survive rebuilding
and return SQL NULL distances in both modes. A formatting-only printf control
had misleadingly rendered NULL as 0.0. Retain typed product decoding and errors,
and test nonfinite values, damaged packed blocks, failed rebuild rollback and
old pinned readers. Full-K distance equality on valid data alone does not prove
corruption parity. Native rollback unloads its model, but cursor configuration
can be retained: finish mode-inspection statements before rebuilding and use fresh
observations after rollback/retry rather than keeping active cursors across it.

[Raw typed corruption controls](../../docs/zkit/letopis/checks/issue-65/vector-index/native-probe/control-v2-runs.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/native-probe/control-v2-runs.json")

[Caught production mutants](../../docs/zkit/letopis/checks/issue-65/vector-index/candidate/mutations.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/candidate/mutations.json")

## Bounded SQL retrieval after the index experiment {#bounded-sql}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

D-108 now uses L=min(requested limit or 100,100) for both SQL channels and final
output. FTS and vector SQL return at most L knowledge hits after ordering/joins;
Rust checks only their bounded union, without filter refill. Native K stays full
inside SQLite for current-node coverage and URI ties; this is not an ANN or vector
storage migration. On the same real-vector 100k corpus, three pairs measured
66.622→1.344s at L5 and70.915→1.867s at L100; this supersedes the old index-only
whole-search result as the latest query measurement. The old native speedup and
population/write-layout limitations remain historical facts, not new drain data.
Original and owned100k payloads retain all307200000 bytes and their ordered hash.

[Bounded SQL measurements](../../docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json")
[Bounded search contract](../../docs/superpowers/specs/2026-11-07-bounded-search-design.md "letopis:type=external;path=docs/superpowers/specs/2026-11-07-bounded-search-design.md")
[Original vector preservation](../../docs/zkit/letopis/checks/issue-65/bounded-search/vector-preservation/summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/vector-preservation/summary.json")
