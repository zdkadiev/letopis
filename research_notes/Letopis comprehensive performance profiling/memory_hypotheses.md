# Letopis memory and allocation hypotheses

## 1. Where can live memory, retained capacity and allocation traffic grow?

### Takeaway

**Memory-only source research, not measurements.** All bottlenecks below remain hypotheses until measured. General CPU/timing/command traces belong to [hot_path_hypotheses.md](hot_path_hypotheses.md), not this note. Inspected revision: `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. No builds, installs, benchmarks, product commands, provider calls, secret/configuration reads, or additional agents. Only this note added; attempted earlier general-note write was cancelled and no `source_hypotheses.md` was saved.

Notation: F = candidate files, N = knowledge nodes, E = edges, B = largest currently read file, A = elements in that file, M = parser events, V = vector keys, D = dimensions, R = output bytes. Counts below are source-level lifetime/scaling observations, **not exact heap formulas**: capacity growth, allocator rounding, native caches and compiler lifetimes affect peaks.

### Cited Findings

| ID | Memory/allocation hypothesis | Concrete lifetime / allocation evidence | Discriminating workload; what would weaken hypothesis |
|---|---|---|---|
| MEM-01 | Publication peak may include whole-cache graph/edge metadata plus current file parse, rather than only changed file bytes. | `get_file_state` retains all file rows in BTreeMap and snapshot roots. `prepare_publication` builds `EffectiveGraph` from complete node Vec and edge Vec; knowledge map clones IDs as keys, metadata retained. Graph lives through cache write/publication, while each `PreparedProjection` owns metadata/search text. [File state](../../src/service/document_refresher.rs#L264-L278), [preparation](../../src/service/document_refresher.rs#L426-L464), [global aggregation](../../src/service/document_refresher.rs#L883-L898), [prepared payload](../../src/service/document_refresher.rs#L62-L96), [write lifetime](../../src/service/document_refresher.rs#L349-L409). | One fixed small edit while unrelated N/E/F and ID lengths grow. Capture live allocator stacks around `EffectiveGraph::from_storage` and during parse. Hypothesis weakened if live bytes flat and native pages dominate. Distinguish total allocations during Vec→map/set conversion from final map/set retained bytes; moved values do not imply full deep copies. |
| MEM-02 | Many candidate paths may create large overlapping raw/filtered/typed path buffers and ordered collections, even when knowledge count small. | Git holds raw tracked path bytes and collected untracked entries, then filtered bytes; CandidateSelection retains raw bytes, selected entries and a filtered byte buffer before `parse_candidates`. Refresher converts candidate Vec to BTreeSet while retaining previous file rows and delta maps. [Git list](../../src/git/repository.rs#L779-L836), [selection](../../src/service/utils.rs#L85-L117), [delta](../../src/service/document_refresher.rs#L281-L317). | Many long unmarked Markdown paths vs fixed marked corpus; tracked/untracked and ignore-mode variants. Look for allocation stacks in path parsing/BTree nodes plus transient byte buffers. Flat peak but high event bytes would weaken *retained* concern, not churn concern. |
| MEM-03 | Large-file parsing may transiently retain raw bytes, whole event stream, layout, edges and all element search strings together. Unmarked files still materialize events. | Filesystem `file_get_content_by_path` uses fresh Vec + `read_to_end`; Markdown borrows UTF-8 text from those bytes, collects offset events before markup detection, and constructs ParsedFile/search strings. `search_text_render` builds String then `trim_matches(...).to_owned()` produces another owned String before original is released. [Capture](../../src/filesystem/local.rs#L583-L613), [parser](../../src/markdown/parser.rs#L45-L79), [render](../../src/markdown/parser.rs#L546-L574). | Same total bytes in many small files vs one huge file; vary A/events/link/code density. Live allocations should rise with largest file/event/text payload if hypothesis important. Do not count borrowed `&str` as another raw-file copy. Parser Event strings may be borrowed or owned; exact split requires allocator evidence. |
| MEM-04 | Repeated captures/parse generate high allocation traffic without necessarily retaining all copies simultaneously. | Detection, basis preflight and SQL preparation each recapture bytes; preflight parsed file is consumed into metadata-only basis, then later SQL preparation reparses. Drain prepares current file anew per missing text; traversal reparses per returned knowledge. [Detection](../../src/service/document_refresher.rs#L320-L346), [preflight](../../src/service/document_refresher.rs#L538-L581), [SQL parse](../../src/service/document_refresher.rs#L670-L717), [drain](../../src/service/embedding.rs#L310-L332), [traversal](../../src/service/traversal.rs#L86-L110). | Many returned/missing elements sharing large owner vs separate small owners. Compare total allocation/free event volume and live high-water separately. Stable peak with growing allocation traffic supports churn, not leak. Never multiply captures by B and claim all such copies coexist. |
| MEM-05 | Search's bounded parse reuse can lower churn yet retain one parsed metadata projection; thresholds do not cap memory needed to parse oversized file. | `CurrentKnowledge` reuses only last successful projection ≤256KiB file, ≤1024 elements, ≤256KiB IDs. Before retention it clears edges and replaces all search strings with empty strings, then shrinks elements. File bytes read/hash anew per candidate; successful parsed projection remains while next candidate is observed. [State/constants](../../src/service/search.rs#L21-L38), [capture/reuse](../../src/service/search.rs#L252-L323). | Just below/above thresholds, same candidate count, adjacent IDs sharing owner vs distinct owners. Compare live retained metadata versus cumulative parse events. Refuted as major retained consumer if small alongside native caches/global refresh. Edge `clear` releases elements; do not assert search text capacities retained after assignment to `String::new()`. |
| MEM-06 | Source fanout may cause substantial repeated whole-file allocation even for tiny line ranges, while bounded historical reuse stores hashes/identities rather than payloads. | `LocalSource::select_bytes` borrows selected slice from already captured entire file. Live provenance captures each binding independently. Batch historical reads materialize one verified blob and compute hashes; cache holds `(tree,source)→Option<Sha256>`, available root set and planned address groups, bounded to 4096 entries and 1MiB logical address admission. SourceService also reads whole source and returns metadata, not source bytes. [Slices](../../src/domain/source.rs#L85-L113), [live reads](../../src/service/provenance.rs#L234-L248), [batch lifetimes](../../src/service/provenance.rs#L199-L231), [cache/plan](../../src/service/provenance.rs#L279-L427), [source reader](../../src/service/source.rs#L24-L63). | Many ranges near EOF on one huge local source; repeated exact address; within/across batch limits; remote-only control. Allocation traffic may grow while only one large live-source buffer exists at a time. Batch cap limits logical keys, not Vec allocator overhead, blob size or total process peak. |
| MEM-07 | Git addressed reads can briefly hold decode buffer and cloned owned result; packed-delta decode may exceed final body size. 4MiB threshold bounds reusable capacity after read, not peak object size. | ObjectReader keeps buffer and up to 4096 validated tree identities, explicitly no object size allocation cap. `object_bytes_get_by_oid` clones decode buffer, then clears/replaces reusable allocation; `object_get_by_oid` only clones trees and discards blob payload after verification. [State/limits](../../src/git/reader.rs#L19-L35), [uncapped decode](../../src/git/reader.rs#L60-L86), [clone/release](../../src/git/reader.rs#L101-L192). | Large addressed loose/packed/delta blobs and wide trees, below/above 4MiB. Live high-water at clone vs retained bytes after read. Weakens hypothesis if decoder/buffer not material relative other owners. Chunked checksum does **not** mean streamed constant-memory ODB decoding: source still fills buffer. |
| MEM-08 | Strong snapshot validation may retain O(reachable object identities) pending/visited state; inspect additionally retains corpus entry/source maps, not all raw corpus bytes. | Closure walk owns `pending Vec<(String,String)>`, `visited BTreeMap` per root and transient verified tree bytes/children. Inspect loops files, merges `CapturedKnowledge` maps/sets, creates entries Vec for tree assembly, then captures grouped sources. [Closure](../../src/git/repository.rs#L578-L633), [captured state](../../src/service/inspection.rs#L19-L25), [file loop](../../src/service/inspection.rs#L49-L83), [source/tree assembly](../../src/service/inspection.rs#L135-L169). | Same bytes with many tiny objects vs few huge blobs; one requested inspect ID while whole corpus grows; deep/wide snapshots. Look for identity/BTree/assembly allocations distinct from decode buffers. Per-root sequential audits do not imply all roots' visited maps remain live together. |
| MEM-09 | Check/status may peak on duplicated cache materialization, validation structures and accumulated problems; path selection does not bound graph-memory preparation. | GraphDiagnostics loads node/edge/file Vecs, builds knowledge/edge BTreeSets and problems before filtering selected paths. Includes validation builds ID-index BTreeMap, outgoing **and** incoming adjacency Vecs, visited/order/stack arrays. Source checking separately loads node Vec and all edges into source-binding map. Graph source buffers can be freed between phases, but problem Vec persists into source checks. [Diagnostics](../../src/service/graph_diagnostics.rs#L38-L74), [source checking](../../src/service/utils.rs#L288-L359), [graph allocations](../../src/domain/validation.rs#L87-L158). | Same selected path with growing unrelated N/E; no includes vs dense includes; many dangling/invalid findings. Distinguish sequential re-materialization traffic from overlapping peak; inspect live stack traces rather than summing each function's maximal state. |
| MEM-10 | Traversal visited/queue/result state may grow with reached nodes/recorded edges despite bounded neighbor pages; page buffers are only one component. | Discovery owns queue Vec, visited BTreeSet, element Vec and optional ordered edge map. Queue retains processed entries until walk ends. Two directional streams each buffer one page. `into_graph` converts edge map to Vec, sorts and collects edge Vec; hydration converts graph elements to links. [State](../../src/service/graph.rs#L19-L38), [recording/conversion](../../src/service/graph.rs#L101-L157), [page buffer](../../src/service/graph.rs#L227-L245), [hydration](../../src/service/traversal.rs#L69-L76). | Large limit, broad reachability, long identities, edge_limit absent vs large; many edges to already visited nodes. Page size 32 is not whole-traversal bound. Small final JSON cannot establish small peak if BFS state already dropped. |
| MEM-11 | Rust search ranking heap is bounded by ≤200 candidate identities, but source fanout, file sizes and native vector/SQL memory are not bounded by that count. | Current L≤100 per SQL channel, union ≤2L; metadata/eligibility/rank BTreeMaps/Sets allocate IDs/links, overlap may create temporary replaced metadata. Source set per candidate can be large; comparison memo bound is 4096. [Current contract](../../docs/zkit/letopis/contracts/X-1-cli.md#L126-L137), [metadata](../../src/service/search.rs#L400-L450), [eligibility](../../src/service/search.rs#L155-L189), [comparison cache](../../src/service/search.rs#L454-L495), [rank allocations](../../src/service/search.rs#L542-L579). | Fixed L, grow unrelated N vs per-candidate source count/file B. If Rust rank allocations flat but peak rises, investigate refresh/native/source buffers rather than claiming all-corpus Rust fusion. |
| MEM-12 | Native vec1/SQLite may have substantially larger working memory than small Rust result vectors; exact allocation size unknown. | Vector query obtains count of **all** keys, passes `K:{count}` to vec1, joins current nodes, sorts distance/URI and SQL-limits result. Query Vec<f32> encoded to owned byte Vec; native extension allocations are outside Rust type accounting. [Query](../../src/storage/vector.rs#L673-L752), [encoding](../../src/storage/vector.rs#L755-L762). | V×D sweep, orphan keys, many nodes sharing one vector, L=1/100. Attribute allocator stacks to vec1/sqlite vs Rust. Native allocation hypothesis weakened if peaks/caches flat. Do not state O(V×D) *query heap*: on-disk payload, resident pages, top-K workspace and full scan bytes are different quantities. Native source allocation strategy not audited here. |
| MEM-13 | Embedding transport may hold JSON text/value representations and decoded f32 vectors together; drain currently processes one text/vector, not huge application batches. | Embedding `compute_vector` calls provider with `&[text]` and expects one result. HTTP provider builds owned JSON Value request, receives response Value, decodes numeric JSON arrays into Vec<Vec<f32>> while response is live. Store encodes vector into byte Vec; SeaQuery conversion moves Bytes→Sqlite Blob rather than necessarily cloning. [One text](../../src/service/embedding.rs#L444-L465), [request/response](../../src/embedding/provider.rs#L242-L263), [JSON/decode](../../src/embedding/provider.rs#L306-L351), [component Vec](../../src/embedding/provider.rs#L378-L387), [store](../../src/storage/vector.rs#L370-L400), [bound conversion](../../src/storage/query.rs#L107-L147). | Controlled D and long text, one deterministic localhost response at a time; separately account provider process. Vec<f32> logical payload is 4D bytes, not complete response/heap bound. Allocation evidence may show request/JSON dominates vector payload. |
| MEM-14 | Drain can retain skip-ID set for whole run plus allocator/native-cache high-water, without retaining all computed vectors/texts. | `drain_ready` owns skipped BTreeSet and cursor for run, prepares/processes one work item per iteration, resets cursor after processing; final prune in vector write. [Loop](../../src/service/embedding.rs#L257-L307), [work lifetime](../../src/service/embedding.rs#L335-L365). | Large stale/missing-owner set vs large valid unique backlog. Watch live skipped IDs vs stable per-iteration vector allocation traffic. RSS that stays high after previous large buffer free is not alone a leak. |
| MEM-15 | Multiple SQLite owners/attachments, page caches, lookaside/schema/statement memory, transient in-memory reference DBs can form significant non-domain heap. | Foreground commands build embedding executor then their own storage group. Each group opens cache writer + vector writer + cache reader/attachments; schema validation constructs ephemeral reference DB; flat model comparison constructs reference vector DB. No `cache_size`, `mmap_size`, `temp_store` override found in inspected `src/`. [Construction](../../src/commands.rs#L63-L81), [open](../../src/lib.rs#L834-L857), [schema reference](../../src/storage/schema.rs#L185-L239), [model reference](../../src/storage/schema.rs#L90-L104). SQLite documents pager caches, lookaside pools, statement/schema allocations; per-connection status can separate some categories. [SQLite allocator](https://www.sqlite.org/malloc.html), [db_status](https://www.sqlite.org/c3ref/c_dbstatus_options.html). | Tiny initialized DB vs populated, foreground vs direct drain/status, large broad SQL sorts/FTS, full publication vs no-op. Malloc stacks should identify sqlite allocators, but lookaside suballocations may appear as one pool. Do not assume default cache values from public docs equal actual bundled build; no runtime PRAGMAs/build flags measured. |
| MEM-16 | JSON response formation may overlap typed result, output-facing collections and complete encoded byte Vec; large input also fully buffered. | `result_build` collects borrowed DTO vectors/maps, links own flag Vecs, inspect hashes produce hex String. Most paths/IDs/details borrow rather than clone; `serde_json::to_vec` materializes full response before output. Typed response is dropped by return from encoding, so slow sink retains bytes but not necessarily original typed graph. [Encode/write](../../src/cli/output.rs#L129-L190), [DTO collections](../../src/cli/output.rs#L193-L237), [per-link/hash](../../src/cli/output.rs#L278-L321), [input](../../src/cli/input.rs#L409-L436). | Large check/status problems, traversal edges, inspect metadata, long IDs; compare R and live-byte phase. High allocation/free traffic during encoding with low post-encode heap supports transient overlap. Do not claim every String deep-copied into DTOs or that slow sink retains all earlier phase buffers. |

Memory coverage by use case: help/version → small argument/help/output allocation baseline; config → config bytes/JSON/provider-construction lifetime; init → transient schema/native setup and config; status → MEM-06/07/09/15/16; every index mode → MEM-01/02/03/04/06/07/08/15; check → those refresh cases plus MEM-09/16; search → refresh plus MEM-05/06/07/11/12/13/15/16; traverse → refresh plus MEM-04/06/07/10/15/16; inspect → refresh plus MEM-03/06/07/08/16; explicit/detached drain → MEM-04/13/14/15. Successful foreground commands additionally retain their task executor until after background availability/startup. [Dispatcher/ownership](../../src/commands.rs#L26-L181), [background path](../../src/service/background_runner.rs#L35-L54).

### Inferences

- A useful conceptual peak decomposition is **simultaneous** domain metadata + current raw/parsed file + result under construction + SQLite/vec1/gix working state + allocator/VM overhead. Adding maximum usage from disjoint phases overstates peak; omitting native allocations understates it. [Lifetimes](../../src/service/document_refresher.rs#L349-L409), [native ownership](../../src/lib.rs#L834-L857), [output](../../src/cli/output.rs#L156-L190).
- No cache schema migration, broad copying of user work, or memory-tuning change is proposed. Read capture, historical-byte verification, last-valid projection and owner/snapshot semantics must survive any later optimization. [Architecture](../../docs/zkit/letopis/architecture.md#L143-L157), [projection contract](../../docs/zkit/letopis/contracts/I-2-projections.md#L18-L20).

### Gaps

No allocation byte counts, retention snapshots, native vec1 allocation audit, mmap residency, concrete SQLite cache settings or allocator implementation verification. Native source path `ext/vec1` referenced by historical docs was absent locally; no substitute source/version inferred. No runtime measurements attempted because compile/profiling work is owned elsewhere.

## 2. What can macOS diagnostics actually establish?

### Takeaway

**Peak RSS ≠ current live heap ≠ allocation traffic ≠ leak.** Use separate evidence streams and report tool coverage. Local installed manuals were read as primary sources; their commands below were **not run against workloads**.

### Cited Findings

| Question / metric | Valid evidence | Limits / invalid inference |
|---|---|---|
| Current memory mappings, malloc zones, dirty/swapped/reserved regions | `vmmap -summary PID`; detailed `vmmap -w PID` or summary from `.memgraph`. Installed manual documents virtual-region sizes and malloc zone allocated vs dirty+swapped. [vmmap manual](file:///usr/share/man/man1/vmmap.1), especially lines 22–32, 65–83, 159–164. | Virtual size is address space reserved, not necessarily physically instantiated. One snapshot is not peak; malloc allocated total not total RSS. File mappings/shared libraries may be shared; do not charge all mapped bytes as private heap. |
| Current malloc allocations / rough ownership stacks | `heap -s -noContent PID`; with MallocStackLogging, `malloc_history PID -allBySize` / `-callTree -invert -ignoreThreads`; archived memgraph also supported. [heap](file:///usr/share/man/man1/heap.1), lines 18–35; [malloc_history](file:///usr/share/man/man1/malloc_history.1), lines 85–104. | `-allBySize` reports **currently live**, not all lifetime allocated bytes. Rust Vec/BTree allocations usually non-object blocks; Objective-C class categorization not reliable Rust type accounting. Attribution depends on actual system-malloc route, symbols/unwinding. |
| Allocation/free event traffic and malloc live high-water | Launch with `MallocStackLoggingNoCompact=1` **alone**, then `malloc_history PID -allEvents`; `-allBySize -highWaterMark` computes live allocations at allocated-memory high-water when full history available. [malloc_history](file:///usr/share/man/man1/malloc_history.1), lines 62–98, 172–180. | Plain MallocStackLogging can compact immediate allocation/free pairs. If both env vars set, MallocStackLogging takes precedence and NoCompact ignored. High-water is logged allocated memory, **not** peak RSS. Historical logs can be huge; event interpretation must handle malloc/realloc/free semantics, not sum all displayed sizes blindly. |
| Unreachable malloc blocks | `leaks -noContent PID`, optionally memgraph; at-exit launch mode captures state as command exits. Scanner treats pointers from writable globals/registers/stack/reachable blocks as evidence of use. [leaks](file:///usr/share/man/man1/leaks.1), lines 34–45, 180–196. | “0 leaks” is not “no excessive memory” or “no retained memory”: reachable caches/over-retention not reported; conservative pointer detection can hide unreachable allocations. A report is a lead, not automatic proof of Rust ownership bug. One-shot CLI drops major state before exit, so at-exit scan misses earlier reachable peak/churn. |
| RSS peak / current resident working set | Separate process-accounting tool with documented units/semantics; use baseline evidence from coordinator, not inferred from malloc logs. `vmmap` can explain region composition at chosen times. [vmmap virtual-size caveat](file:///usr/share/man/man1/vmmap.1). | Freed allocator pages may stay resident/reusable; resident file pages may not be malloc; shared and compressed pages complicate totals. No allocation count, allocation stack, or lifetime traffic from RSS alone. Sum of process peaks is not simultaneous process-tree peak. |
| SQLite-only current heap and categories | Future instrumented same-process harness could call `sqlite3_memory_used/highwater` and `sqlite3_db_status` for CACHE_USED, SCHEMA_USED, STMT_USED, lookaside statistics on **actual owners**. [SQLite memory](https://www.sqlite.org/malloc.html), [connection counters](https://www.sqlite.org/c3ref/c_dbstatus_options.html). | New SQLite CLI/process cannot inspect another process's allocator counters. CACHE_USED/SCHEMA_USED/STMT_USED highwater fields always zero; sample current counters or use appropriate global high-water. Native extension/system allocations outside SQLite allocator may be excluded; these are not RSS. No instrumentation added in this task. |

Security/observer effects:

- `leaks -fullContent` / `malloc_history -showContent` can expose contents; use noContent and never target personal apps/provider processes. Even noContent memgraph retains addresses/references/backtraces and paths; keep evidence local. [leaks content warning](file:///usr/share/man/man1/leaks.1), lines 121–147; [malloc_history content](file:///usr/share/man/man1/malloc_history.1), lines 153–155.
- Malloc stack logging, event retention, process inspection and snapshots perturb allocation/VM/timing. Keep logging runs separate from normal resource baseline and samply CPU runs. They establish attribution, not uninstrumented latency/footprint. Logs use memory/storage; do not mistake observer overhead for product-state growth. [Logging/event modes](file:///usr/share/man/man1/malloc_history.1), lines 95–98, 172–180.
- Parent foreground, detached drain and deterministic loopback server are distinct processes/owners; capture each explicitly. Product starts drain with detached spawn and does not await completion. [Background](../../src/service/background_runner.rs#L36-L54), [composition](../../src/lib.rs#L642-L656). Do not assume parent log covers child; inherited logging variables may instrument child, but attachment/archival must still target its PID.

### Inferences

Strong allocation explanation combines: unlogged peak resident evidence, logged high-water live allocation stacks, current live retained allocation stacks, and (for churn) complete event history. Flat live heap + high allocation traffic is compatible with healthy per-file release; rising RSS alone cannot choose leak versus allocator retention versus native cache. [Apple allocation/live/event distinction](file:///usr/share/man/man1/malloc_history.1), [VM caveat](file:///usr/share/man/man1/vmmap.1), [reachability scanner](file:///usr/share/man/man1/leaks.1).

### Gaps

Attach permissions, code signing/hardened-runtime constraints, symbols, operating-system-specific output details, logging support and actual allocator route are untested. If attachment fails, preserve error and stop; do not disable SIP, alter signing/system security, or elevate automatically. Local manual rendering via `/usr/bin/groff` was unavailable, so raw installed man sources were read instead; no tool installation attempted.

## 3. What practical diagnostic runs should follow, without changing product code?

### Takeaway

Commands below are **future operator recipes, not executed evidence**. Use an already-built immutable executable, disposable synthetic Git fixture and fresh evidence directory. Long work inside one process is needed for meaningful live attachment; a shell loop launching many short commands is not a long-lived Letopis heap.

### Cited Findings

The normal CLI is process-per-call; full index, full check/inspect or a large explicit embedding backlog can extend one command's lifetime. Direct drain processes published work without refresh; a deterministic provider's delay can hold the process alive but often between allocations, not at memory peak. [CLI lifecycle](../../docs/zkit/letopis/architecture.md#L30-L32), [full indexing](../../src/service/document_refresher.rs#L214-L221), [drain loop](../../src/service/embedding.rs#L257-L333).

Provider construction reads `OPENAI_API_KEY` from environment; compatible protocol requires printable key even for localhost. Use clean allowlisted environment and **explicit fixture-only dummy** key solely for verified loopback fixture; never inspect/forward real key/config. [Construction](../../src/lib.rs#L691-L701), [compatible validation](../../src/embedding/provider.rs#L242-L270). For publication/check/inspect memory controls use disabled embeddings to avoid child provider/drain allocation interference.

### Inferences

#### A. Live allocation/VM snapshot of a large disabled-embedding full index

Run from an interactive operator shell, substituting paths; fixture/config must already exist and be synthetic. `bin` must point to stable completed build/copy, **not executable being replaced by concurrent compile**. Use unique new evidence directory, with local-only permissions. Recipe targets full index; replace argv/input for long check/inspect/traverse as needed.

```zsh
bin="/absolute/path/to/already-built/letopis"
fixture="/absolute/path/to/disposable/synthetic-git-fixture"
evidence="/absolute/path/to/new/memory-evidence-run"
umask 077
mkdir -p "$evidence/home" "$evidence/tmp"
print -r -- '{}' > "$evidence/request.json"
cd "$fixture"

# Logging run, not normal RSS/latency baseline. No personal env/config inherited.
# NoCompact alone preserves immediate allocation/free pairs.
/usr/bin/env -i PATH=/usr/bin:/bin HOME="$evidence/home" \
  TMPDIR="$evidence/tmp/" MallocStackLoggingNoCompact=1 \
  "$bin" index --full < "$evidence/request.json" \
  > "$evidence/command.json" 2> "$evidence/command.stderr" &
pid=$!
print -r -- "$pid" > "$evidence/pid.txt"

# Issue while that exact process is still alive; each snapshot perturbs target.
/usr/bin/vmmap -summary "$pid" > "$evidence/vmmap-summary.txt" 2>&1
/usr/bin/heap -s -noContent "$pid" > "$evidence/heap.txt" 2>&1
/usr/bin/malloc_history "$pid" -allBySize \
  > "$evidence/live-by-stack.txt" 2>&1
/usr/bin/malloc_history "$pid" -allBySize -highWaterMark \
  > "$evidence/high-water-by-stack.txt" 2>&1
/usr/bin/leaks -noContent -fullStackHistory \
  -outputGraph "$evidence/state.memgraph" "$pid" \
  > "$evidence/leaks.txt" 2>&1

wait "$pid"
code=$?
print -r -- "$code" > "$evidence/command-exit.txt"
```

These options come from [vmmap](file:///usr/share/man/man1/vmmap.1), [heap](file:///usr/share/man/man1/heap.1), [malloc_history](file:///usr/share/man/man1/malloc_history.1) and [leaks](file:///usr/share/man/man1/leaks.1) installed manuals. The script is a recipe, not an automated trustworthy capture pipeline: process may exit between snapshots, zsh `errexit` can affect `wait`, and each diagnostic has its own exit status. Retain diagnostics/errors, verify PID is task-owned before attachment, and mark partial snapshots unavailable rather than report them as successful.

No sleeps or signal-based pausing required. Increase bounded synthetic work if process too short. Do not artificially hold a transaction/open stdin and call that production peak. `SIGSTOP`/debugger breakpoints are not proposed: they can interfere with locks and observational tooling.

#### B. Offline attribution and churn from archived full stack history

```zsh
/usr/bin/vmmap -summary "$evidence/state.memgraph" \
  > "$evidence/offline-vmmap.txt" 2>&1
/usr/bin/malloc_history "$evidence/state.memgraph" -callTree -invert -ignoreThreads \
  > "$evidence/offline-live-calltree.txt" 2>&1
/usr/bin/malloc_history "$evidence/state.memgraph" -allBySize -highWaterMark \
  > "$evidence/offline-high-water.txt" 2>&1

# Optional: potentially enormous; prefer separate modest attribution fixture first.
/usr/bin/malloc_history "$evidence/state.memgraph" -allEvents \
  > "$evidence/offline-all-events.txt" 2>&1
```

`-fullStackHistory` archives available freed-allocation history; no snapshot can reconstruct events that were never logged or after snapshot time. [leaks archive mode](file:///usr/share/man/man1/leaks.1), lines 145–147; [malloc_history history requirements](file:///usr/share/man/man1/malloc_history.1), lines 62–98. Archive before process exits; do not depend on automatic post-exit log availability. Avoid `-showContent`.

#### C. At-exit leak control for short commands

```zsh
# Same synthetic fixture/clean environment; diagnostic output mixed with command
# stdout here, so this file is NOT the normal one-JSON contract capture.
/usr/bin/env -i PATH=/usr/bin:/bin HOME="$evidence/home" \
  TMPDIR="$evidence/tmp/" \
  /usr/bin/leaks -noContent -atExit -- "$bin" status \
  < "$evidence/request.json" \
  > "$evidence/leaks-at-exit-combined.txt" 2>&1
```

`leaks -atExit` automatically sets MallocStackLogging=lite unless overridden. Use it for reported unreachable allocation leads, not full allocation traffic/high-water accounting. Exit 0 means no leaks detected, 1 means leaks detected, >1 tool error; not interchangeable with captured Letopis JSON success. [Installed leaks manual](file:///usr/share/man/man1/leaks.1), lines 180–219.

#### C2. At-exit memgraph with explicit stack logging

For commands too short to attach reliably, installed syntax supports combining an output graph with at-exit execution. Use another fresh evidence directory for this separate diagnostic run:

```zsh
# Explicit YES overrides leaks' automatic lite setting. Do NOT also set NoCompact:
# installed malloc_history manual says MallocStackLogging takes precedence.
/usr/bin/env -i PATH=/usr/bin:/bin HOME="$evidence/home" \
  TMPDIR="$evidence/tmp/" MallocStackLogging=YES \
  /usr/bin/leaks -noContent -fullStackHistory \
  -outputGraph "$evidence/at-exit.memgraph" \
  -atExit -- "$bin" status \
  < "$evidence/request.json" \
  > "$evidence/at-exit-combined.txt" 2>&1

/usr/bin/vmmap -summary "$evidence/at-exit.memgraph" \
  > "$evidence/at-exit-vmmap.txt" 2>&1
/usr/bin/heap -s -noContent "$evidence/at-exit.memgraph" \
  > "$evidence/at-exit-heap.txt" 2>&1
/usr/bin/malloc_history "$evidence/at-exit.memgraph" -allBySize \
  > "$evidence/at-exit-live-stacks.txt" 2>&1
/usr/bin/malloc_history "$evidence/at-exit.memgraph" -allBySize -highWaterMark \
  > "$evidence/at-exit-allocation-high-water.txt" 2>&1
```

The installed manuals support each option and archived analysis; combined recipe remains **unexecuted**. Require graph creation and successful tool analysis before treating it as evidence. Full stack history preserves **available** history, not events removed by compact logging. `YES` provides full rather than lite mode, but immediate allocation/free pairs can still be compacted; this recipe does not establish complete churn. Use direct NoCompact launch from A/B for that question. At-exit live heap describes shutdown-time survivors, not earlier graph/parser retention; a usable historical allocated high-water can recover logged allocation stacks for an earlier peak, **not RSS at that peak**. [leaks full history/at-exit](file:///usr/share/man/man1/leaks.1), lines 145–147, 180–196; [malloc_history high-water/log modes](file:///usr/share/man/man1/malloc_history.1), lines 62–98, 172–180.

#### C3. Trace a suspicious retained allocation without dumping contents

Select a graph from one live-stage capture or at-exit run. List addresses without contents, then choose one **address actually listed in that same graph**:

```zsh
graph="$evidence/state.memgraph"
/usr/bin/heap -s -noContent -addresses 'malloc[1m+]' "$graph" \
  > "$evidence/large-allocation-addresses.txt" 2>&1

# Set manually from preceding output; placeholder is NOT an address to execute.
address="0xREPLACE_WITH_ADDRESS_FROM_THIS_GRAPH"
/usr/bin/malloc_history "$graph" "$address" \
  > "$evidence/selected-allocation-history.txt" 2>&1
/usr/bin/leaks -noContent -traceTree "$address" "$graph" \
  > "$evidence/selected-reference-tree.txt" 2>&1
/usr/bin/leaks -noContent -referenceTree -groupByType "$graph" \
  > "$evidence/reference-summary.txt" 2>&1
```

`heap` documents `malloc`, `non-object` and size-range patterns; if type grouping does not match Rust buffers, use `-addresses all` with `-noContent`, then select by reported size. `malloc_history <graph> <address>` identifies allocation/free stacks for a containing block where history exists. `leaks -traceTree` shows a reverse reference tree toward roots; `-referenceTree` summarizes all malloc/anonymous VM nodes once. Attribution is heuristic: pointer-like stale values and multiple references can put a block under the wrong apparent parent. TraceTree is **not** proof of precise Rust ownership, and stack attribution describes where memory was allocated, not necessarily who currently owns it. [heap patterns](file:///usr/share/man/man1/heap.1), lines 84–119; [address history](file:///usr/share/man/man1/malloc_history.1), lines 75–92; [reference modes/caveat](file:///usr/share/man/man1/leaks.1), lines 150–167.

Use actual installed **single-dash** options (`-noContent`, `-atExit`, `-outputGraph`), not assumed GNU `--noContent`/`--atExit` forms. `--` is the separator after `-atExit`. No content display flags, `MallocScribble`, security changes, executable rebuilds or installations are needed for these procedures. Summaries/stacks/addresses may still disclose fixture paths and structure: synthetic workloads only, local restricted evidence.

#### D. Memory comparisons that isolate the hypotheses

- **Peak vs churn:** same total corpus bytes, vary largest file and elements/file; unlogged peak resident run separate from NoCompact event attribution run. MEM-03/04/05/13.
- **Retained graph vs file buffer:** hold changed file fixed, grow unrelated N/E/F and identity lengths; capture around publication, not only after return. MEM-01/02/09.
- **Source bytes vs range count:** fixed large source, many tiny ranges; compare logged cumulative allocated bytes, live high-water and memgraph live buffers. Grouped historical range hashes are not retained source payloads. MEM-06/07.
- **Native pages vs Rust results:** V/D/current/orphan coverage sweep with fixed L; use native allocation stacks/VM categories, never claim all RSS is Vec memory. MEM-11/12/15.
- **Drain bounded work state:** large unique backlog with one text per request, then stale-work run; distinguish per-iteration parser/JSON/vector churn from growing skip IDs. MEM-04/13/14.
- **Renderer phase:** large successful graph/findings result, record R and high-water allocation stacks; current post-output live heap understates transient encode overlap. MEM-16.
- **Lifecycle:** one long process for retention investigation; separately many process invocations to characterize aggregate machine load, never label shell-loop RSS as one Letopis leak. Parent/drain/provider memory recorded per PID and, if needed, synchronized simultaneous totals.

All scenarios remain proposed falsification tests, not results. Preserve last-valid projection, no-follow file checks, immutable historical verification, bounded candidate semantics, scoped owner guarantees and no-provider-inside-write invariants. [Read scopes](../../docs/zkit/letopis/contracts/I-8-read-view.md#L54-L80), [projection contract](../../docs/zkit/letopis/contracts/I-2-projections.md#L18-L20).

### Gaps

Recipes were not executed or validated against a live process; availability/permissions/symbol quality remain unknown. Full command/workload fixtures, baseline peak-RSS method and profiling execution remain coordinator-owned. No cross-platform memory claims, no proof of leak absence, and no allocation profiling promised from samply CPU samples or RSS alone.
