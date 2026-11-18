# Letopis performance workloads and safe corpus construction

Research only, 2026-11-17. No builds, scripts, benchmarks, profiler runs, installs, model requests, or product changes performed. Initial local Git status: `main...origin/main`, unrelated untracked `reports/` preserved. Proposals below are not executed measurements or accepted implementation decisions.

## 1. Which existing workloads and tools should be reused?

### Takeaway

Substantial deterministic generation, process measurement, Samply profiling, reset, comparison, and provenance tooling already exists. Reuse it rather than create a second general harness; existing coverage is not comprehensive for current public commands, arbitrary graphs, or OpenAI-only enabled embeddings. [Sources: performance README](../../tests/performance/README.md), [scenario implementation](../../tests/performance/scenarios.py), [current command help](../../src/cli/help.rs).

### Cited Findings

- `tests/target_corpus/generate.py` exposes `CorpusOptions(seed, knowledge_count, fragment_bytes, elements_per_file)` and `generate_corpus(output, options)`. It initializes a standalone, non-bare SHA-1 Git worktree, commits source files with fixed author/date, captures the source tree, and writes marked knowledge plus a manifest. Despite README wording “bare Git fixture,” implementation is a normal worktree. Knowledge files are not committed by this generator. [Source](../../tests/target_corpus/generate.py).
- Defaults: 100,000 knowledge elements; exactly 2,000 UTF-8 bytes per complete marked fragment; 30 elements/file; source files contain up to 100 source lines. Each knowledge has one distinct line-addressed source and four distinct internal edges, totaling five edges. The internal graph is locally connected with wraparound `depends_on` edges; every group of four has a short forward `includes` chain. It is not a deep containment/high-degree benchmark. [Source](../../tests/target_corpus/generate.py).
- Independent `validate.py` checks input SHA-256, metadata/fragment layout, source tree, sources, graph and byte statistics without product/model calls. Its regular expressions and graph expectations deliberately describe this generator, not arbitrary graphs, invalid syntax, or historical variants. Do not reuse its success label for rewritten fixtures without a new independently specified oracle. [Source](../../tests/target_corpus/validate.py).
- `scenarios.py` generates seed 65 fixtures, sanitizes ambient `GIT_`, `LETOPIS_`, `OPENAI_`, `OLLAMA_` environment, configures disabled embeddings, and prepares independent copied/reset samples. Existing scenarios: search, inspect, traverse, check, status, one/two/all-file refresh, historical scenario name `reindex` which now invokes `index --full`. Mutation changes all `Synthetic case` tokens in selected files, so one changed file normally changes 30 knowledge search texts, not one knowledge element. [Source](../../tests/performance/scenarios.py).
- `measure.py`/runner provide fresh child processes, bounded output capture/hash, watchdogs, owned process-group cleanup, wall time and per-child wait4 CPU/lifetime RSS. Report documentation explicitly excludes simultaneous whole-tree/model peak RAM, exact SQL/lock wait, exact parsed/hashed bytes, and I/O-byte claims. OS page cache is uncontrolled. [Source](../../tests/performance/README.md#metric-scopes-and-tests).
- `profile.py` and `window.py` provide direct Samply capture and separately censored windows; `compare.py` freezes build inputs, executable and symbols and alternates before/after cases. Profiles use save-only 1,000 Hz, no browser/upload, validation of usable samples, and three paired unprofiled/profiled runs. Short commands may have insufficient samples; wrappers and CLI PID must remain distinguished. `controls.py` varies external declarations or packing; `historical.py` implements shared, distinct flat, distinct sharded sources at 10k/100k. [Source](../../tests/performance/README.md).
- `contention.py`, native `target_corpus` observer, and existing lock-control tooling establish owned drains, barrier-proven overlap, current vector coverage and query canary participation. Python SQLite is only for ordinary read-only metadata, not native vec1. `cli_runtime_fixture.rs` is explicitly process-mechanics scaffolding, not product embedding evidence. [Sources: performance plan](../../docs/superpowers/plans/2026-10-30-issue-65-performance.md#task-5-fake-vector-and-controlled-contention-scenarios), [runtime fixture](../../examples/cli_runtime_fixture.rs).
- Current public commands are `init`, `check`, `inspect`, `search`, `traverse`, `index`, `status`, `config`; internal drain is `embed --drain`. `index` supports incremental, `--verify`, `--full`, and scoped paths. `check paths` filters findings, not refresh; status does not refresh. Search clamps limit above 100; traverse exposes mode/direction/depth/limit/optional edge_limit. Existing generic scenarios omit init/config/help/version, explicit incremental/verify/scoped index variants and most query/traversal parameter combinations. [Source](../../src/cli/help.rs).

### Inferences

Proposed execution order after authorization:

1. Reuse generator + validator for canonical baseline; existing `run.py`, `measure.py`, `report.py`, `profile.py`, frozen receipts, observer, and contention ownership.
2. Inventory uncovered cases against current CLI; add only narrowly required declarative scenarios/fixtures later, not a duplicate harness.
3. Treat `docs/zkit/letopis/checks/**/bench.py` and captured experiment drivers as historical recipes, not portable turnkey entry points: retained hybrid bench hardcodes local binary paths and exact SHA-256. [Example](../../docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/bench.py).
4. Cover all commands at smoke/1k first, then scale only meaningful operations. Do not cross-product every corruption, topology, embedding dimension, query, and concurrency setting.

### Gaps

- No runtime inventory of currently available compiler, Samply, binaries, free disk, permissions, or tool versions was performed.
- Existing OpenAI-only enabled performance fixture is missing: current fake server implements native Ollama route only, not `/embeddings`. [Source](../../tests/performance/provider.py).

## 2. What valid, realistic corpus families and load matrix should be used?

### Takeaway

Use 60-node smoke, then 1k/10k/100k baselines with orthogonal graph/source/file-shape controls. Keep exact byte sizes for the existing generator family, but use separately labelled content-realistic and malformed fixtures rather than padding claims or stale manifests. [Basis: generator](../../tests/target_corpus/generate.py), [Markdown contract](../../docs/zkit/letopis/contracts/X-4-markdown.md).

### Cited Findings

Valid knowledge syntax:

```markdown
## Cache invalidation {#cache}
<!-- letopis: {"created_at":1792792800,"source_snapshot":"FULL_TREE_OID"} -->
Cache invalidation follows a source version.
[implementation](../sources/cache.rs#L2-L4 "letopis:type=external;path=sources/cache.rs#L2-L4")
[related](part-0001.md#node-000030 "letopis:type=depends_on;path=knowledge/part-0001.md#node-000030")
```

The OID placeholder must be replaced with an actual complete tree OID before parsing. `created_at` is Unix seconds. File-element metadata must occupy the first line and precludes other elements; section metadata follows its explicit anchored heading immediately. Elements cannot nest. Code-block examples are not declarations. Anchor grammar starts with a letter and uses letters/digits/underscore/hyphen. Exact element hash is SHA-256 of raw element bytes, including metadata; search hash excludes metadata/marked titles. [Source](../../docs/zkit/letopis/contracts/X-4-markdown.md).

- Canonical `path` in a link title is Git-root-relative; Markdown destination resolves from the document directory. Only file portions must match after destination normalization; destination fragments are not compared to canonical anchors/ranges. Mandatory title grammar is `letopis:type=TYPE;path=ADDRESS`; `path` is terminal. Self-loop invalidates a file; ordinary relation cycles and parallel edges of different types are allowed. Includes cycles/dangling edges are diagnostic cases. Invalid files retain their last valid projection; fresh invalid files do not publish partial knowledge. [Source](../../docs/zkit/letopis/contracts/X-4-markdown.md).
- Source ranges are inclusive one-based lines and hash exact bytes; sources are not knowledge unless independently marked. Git source basis is a full tree OID, not a commit OID, content SHA-256, or SQL revision. Source snapshot and inspect snapshot have different roles. Current versus addressed historical bytes determine source_changed; missing current file/range determines source_not_found. URL sources never fetch. [Sources: source mechanics](../../docs/zkit/letopis/mechanics.md#mc-4--источники-и-актуальность), [Git contract](../../docs/zkit/letopis/contracts/X-3-git.md).
- Inspection captures all marked knowledge and available declared local sources, even when one requested element is inspected. Source tree breadth, source file size, packing, and first versus reused captured objects therefore matter independently of requested result count. [Source](../../docs/zkit/letopis/contracts/X-3-git.md#verification-boundary--2026-10-29-implementation-update-2026-10-31).
- `.md`/`.markdown` candidates include tracked and eligible untracked files, including unmarked documents which still get `files` rows. Standard Git ignores normally exclude untracked paths, not tracked ones; `.letopis/ignore` applies to both; discovery can disable standard ignores. [Sources: Markdown contract](../../docs/zkit/letopis/contracts/X-4-markdown.md), [current CLI](../../src/cli/help.rs).

### Inferences

The following are **proposed exact workloads**, not established production distributions:

| Family | Exact scale/design | Purpose |
|---|---|---|
| Baseline B | N=60/1,000/10,000/100,000; seed 65; 2,000 bytes/element; 30/file; 5N edges; N source addresses | Existing reusable reference |
| Packing P | N=1k/10k; 10,30,50 elements/file | File discovery/parsing/capture overhead; record changed IDs/includes at file boundaries |
| Shallow containment G1 | N=1k/10k/100k; disjoint 100-node stars; each hub includes its 99 leaves; 0 other internal edges | Broad shallow metadata topology, exact internal edges 0.99N |
| Deep containment G2 | N=1k/10k/100k partitioned into 100-node chains; optional one N-node chain only at 1k then 10k | Depth scaling without making every normal-scale probe adversarial; separate stack/diagnostic stress |
| High degree G3 | One hub related_to every other node: N−1 edges; remaining nodes form forward relation ring | Hub fan-out/incoming traversal, ordinary cycles; ensure ring/hub triples distinct and no self-loop |
| Dense G4 | N=1k/10k only; each node depends_on next 32 nodes modulo N | Exactly 32N internal edges; bounded degree stress, not full O(N²) clique |
| Provenance H | Existing `shared`, `distinct`, `distinct-sharded` 10k/100k | Source address cardinality and tree shape; do not mistake cross-variant absolute times for isolated optimization |
| Source fan-out S | 10k nodes; one source/file shared by all versus 100 files of 100 lines versus 10k distinct files; add 64 KiB and 1 MiB source-file cases at 1k | Separate source files, unique addresses, addressed bytes, captured whole bytes |
| Content C | N=1k/10k; 70% prose/runbooks,20% fenced Rust/SQL examples,10% tables/lists; deterministic English/Russian text; 1,024/8,192/65,536 byte target payload bands | Parser/FTS throughput and long-element cost; counts/actual bytes, not generator exact-size claim |
| Discovery D | 1k marked nodes plus 0/10k/100k unmarked Markdown; paired same-count non-Markdown files; 50% eligible untracked/50% tracked in each population | Candidate scanning overhead distinct from knowledge count |
| Fault F | On valid 1k/10k baseline mutate exactly 1% nodes (10/100), separately stale source bytes, missing source ranges/files, unknown valid tree OID, removed source_snapshot | Provenance flags/filtering; preserve old basis and independently list expected affected IDs |
| Parse-invalid I | Exactly 1 or 10 invalid files at 1k/10k; separate duplicate anchor, invalid JSON, nested element, missing path, destination/path mismatch, self-loop | Fresh-invalid and valid-then-invalid must be different fixtures; last-valid retention oracle |
| Graph diagnostics J | Exactly one dangling target, one two-node includes cycle, and ordinary two-node relation-cycle control in separate fixtures | Check/status graph findings, bounded traversal behavior |

Baseline exact derived sizes:

| Knowledge N | Markdown files | Source files | Distinct source addresses | Edges | Marked UTF-8 bytes |
|---:|---:|---:|---:|---:|---:|
| 60 | 2 | 1 | 60 | 300 | 120,000 |
| 1,000 | 34 | 10 | 1,000 | 5,000 | 2,000,000 |
| 10,000 | 334 | 100 | 10,000 | 50,000 | 20,000,000 |
| 100,000 | 3,334 | 1,000 | 100,000 | 500,000 | 200,000,000 |

Counts derive directly from generator packing, SOURCE_LINES=100 and 2,000-byte fragments; total disk will be larger due to sources/Git/SQLite/vectors/copies. For G1–G4 choose one external edge per node, same source population and ordinary body bytes; total edges = specified internal edges + N. Publish a new manifest because baseline validation/oracles no longer apply.

Command/load matrix:

- Startup: global help/version and per-command help once at smoke and across repeat processes; init absent versus existing state; config absent/valid disabled/valid enabled/invalid. Lifecycle/setup outside ordinary search timing.
- Index: empty initial publication, unchanged incremental, `--verify`, `--full`; scoped one file/directory; one element edit, one file edit, two files, 1% files, all files; insertion/deletion/rename separately. Use exact affected-file and node counts.
- Search: empty, no-hit, unique token, common token; limits 1/10/100; sources filter 1/10/100 URIs; needs_recheck true/false; source_revisions 1 and 4 real trees. Both ordinary and stale/missing sets; expected IDs not just ok:true.
- Inspect: batches 1/10/100 knowledge IDs, source IDs and ranges; first capture versus repeated capture; unknown ID rejects whole batch.
- Traverse: depth 0/1/2/8/64; limits 20/100/1,000; no edge_limit versus 40/1,000; containment/relations/both; incoming/outgoing/both. Apply focused subset per graph: chain depth, star fan-out, ring dedup, and sources.
- Check/status: clean, provenance faults, parse faults, dangling/cyclic graph. Full check versus nonempty check paths; record that check paths does not scope refresh and status never refreshes.
- Enabled embeddings: separately backlog zero/10%/100%, populated coverage, exhausted attempt budget, unchanged text/vector reuse, query failure fallback, config identity reset; direct drain only. Repeat contention at 1/2/5/10 readers only after single-process baseline and ownership smoke pass.

### Gaps

- Arbitrary topology/content/fault fixtures and comprehensive command matrix are proposals; existing runner does not automatically implement them.
- No empirical throughput or size guarantees for these proposed variants. Dense/deep/high-output cases need staged limits, not assumptions of safe completion.

## 3. How should setup/reset, embedding isolation, profiling and budgets remain safe?

### Takeaway

Only new synthetic standalone repositories may be indexed. Enabled OpenAI-only offline profiling needs loopback OpenAI-compatible protocol scaffolding, not the existing native-Ollama performance fake; real OpenAI calls and ambient credentials are excluded. Existing timing history supplies budget warnings, not current performance predictions. [Sources: embeddings contract](../../docs/zkit/letopis/contracts/X-2-embeddings.md), [provider implementation](../../tests/performance/provider.py), [historical 100k results](../../docs/zkit/letopis/checks/issue-65/2026-11-06-100k/report.md).

### Cited Findings

- Provider `openai` means OpenAI-compatible protocol, not a mandatory hostname. It appends `/embeddings` to explicit parameters.base_url, sends model/input/encoding_format:float, optionally dimensions, and validates unique data indexes and finite consistent dimensions. OPENAI_API_KEY is process environment only. Config wraps mandatory enabled/provider/model/parameters/max_retries in embedding_model. Disabled config is locally validated and preserves vectors/failures while using FTS. [Source](../../docs/zkit/letopis/contracts/X-2-embeddings.md).
- Existing performance fake binds numeric loopback with ephemeral port, implements only `/api/embed`, accepts model `performance-fixture`, and returns dimension-3 canary vectors. It records handler CPU, not separate server RSS. Rust embedding tests already exercise OpenAI-compatible indexed responses and synthetic credentials in isolated child processes; those fixtures are reusable protocol examples, not currently a Python performance endpoint. [Sources: fake provider](../../tests/performance/provider.py), [compatible protocol test](../../tests/embedding_provider.rs).
- Cache and vector storage are separate. Queue is derived from knowledge search hashes absent from vector_keys and not exhausted in embed_failures; retries are reserved before HTTP. Identical texts share vectors. Changed identity clears vector state; enabled toggle alone does not. Rebuilding cache alone preserves vectors. Drain rereads current files and skips changed search hashes; successful empty drain does not itself prove complete vector coverage. [Source](../../docs/zkit/letopis/mechanics.md#mc-3--embeddings-работа-из-данных-и-попытки).
- Enabled foreground commands may launch detached `embed --drain`. Existing orchestration suppresses this with an interoperable owned embed.lock lease before first enabled foreground work, then releases and directly owns drain. Reader cleanup must precede writer release to avoid successors. [Source](../../docs/superpowers/plans/2026-10-30-issue-65-performance.md#task-5-fake-vector-and-controlled-contention-scenarios).
- Copied databases/files have changed inodes; existing setup refreshes copied projection before applying the measured edit. Never copy an active SQLite/WAL state and assume a clean equivalent snapshot. Measurements distinguish initialization, initialized projection, first inspect capture, reused inspect objects, profiled series, and censored windows. [Sources: scenarios](../../tests/performance/scenarios.py), [performance README](../../tests/performance/README.md).
- Rust is pinned to 1.95.0, edition 2024; Make checks are locked/offline. `make check` builds all targets and root tests; `make verify` adds strict Rustdoc and release tests. Dist profile uses LTO, panic abort and strip=true; existing profiling builds use release with debug=1 and preserve symbols instead. [Sources: toolchain](../../rust-toolchain.toml), [Cargo](../../Cargo.toml), [Makefile](../../Makefile).
- Historical host Python3 was too old; performance tooling requires Python≥3.11 and recommends `/opt/homebrew/bin/python3.12 -B`. Bare-python subprocess tests also require PATH correction. Historical sandbox verification recorded loopback-bind/process-inspection permission errors and 33 library test failures; these are not demonstrated current compile failures. A stale deliberately mutated executable in shared Cargo target contaminated an earlier test run; separate target directories were required. [Sources: README](../../tests/performance/README.md), [verification history](../../docs/zkit/letopis/checks/issue-65/current/report.md).
- Captured November 6 100k disabled-embedding fixture initialized in 162.648 s, 154.485 CPU s, 431.45 MiB CLI lifetime RSS; first inspect 52.522 s; initialized search/traverse medians ~0.702 s, inspect 8.260 s, check/status ~40.95/40.70 s, full index 207.402 s (n=1). These are older revision, specific host/corpus results, not predictions for current code. Fresh-copy resets were sufficiently expensive that the campaign switched to stable verified repositories for nonmutating read series. [Source](../../docs/zkit/letopis/checks/issue-65/2026-11-06-100k/report.md).

### Inferences

Proposed safe construction/reset contract:

1. After approval, allocate a new normalized absolute absent path under the approved session temporary root or a new ignored evidence directory. Reject symlinks, input/output overlap and existing paths. Do not copy/index this worktree, its `.letopis`, another user's repository, or actual secrets. Synthetic sources and knowledge only.
2. Clear Git/provider environment; disable global/system Git config, hooks, signing, external attributes/filters; fixed seed 65, timezone UTC, fixed author timestamp; standalone `.git`, no remotes/alternates/submodules. Record corpus bytes/counts/hash manifest, source tree, tool versions, binary/build/harness hashes.
3. Snapshot source tree before rendering knowledge. Keep old tree immutable during source-stale cases; source edits affect only synthetic working files. Create additional known trees for source_revisions filters. Use local Git as fixture/oracle only; product uses in-process gix.
4. FTS-only OpenAI config proposal: `{"embedding_model":{"enabled":false,"provider":"openai","model":"performance-fixture","parameters":{"base_url":"http://127.0.0.1:1/v1"},"max_retries":0}}`. Existing `prepare_fixture` writes disabled Ollama config, so strict “OpenAI only” requires a narrowly reviewed setup override before the first product command. Disabled mode must issue no HTTP.
5. Enabled fixture proposal: same config with enabled=true and owned numeric-loopback ephemeral `/v1` base URL; synthetic key only, never real account credentials. Implement/adapt `/v1/embeddings` route from existing Rust protocol fixtures only after approval. No DNS, forwarding, redirects, external API/model inventory/downloads. If “no network” includes loopback sockets, run disabled-only and mark enabled/vector HTTP cases unavailable rather than silently use a different transport.
6. Use dimension 3 for protocol/coverage smoke; for realistic flat-vector scan memory use synthetic finite deterministic vectors at dimensions 384/768/1536, starting 1k then 10k. Raw f32 payload math at 100k is 153.6/307.2/614.4 MB respectively, before index/SQLite/keys/WAL overhead; this is arithmetic, not measured RAM or disk.
7. Prepare cache-empty, cache-populated/vector-empty, fully covered, 10%-missing and exhausted states in separate newly created fixtures. Prefer product drain/config/text changes over ad hoc SQL insertion/deletion. Do not bypass config identity or vec1 bootstrap. Pin ephemeral endpoint/config identity within each enabled fixture; do not pool reset-induced calls with warm-query latency.
8. Reset mutation cases by new copies of quiescent fixtures, then refresh copied inode metadata outside timing. Never reset using broad `rm -rf`, destructive Git, or user repo database deletion. Keep lock-file identities stable while any owned process exists. For read-only initialized series, stable reused private repository is acceptable with pre/post revision/hash/coverage checks; inspect's first durable capture is separate.
9. Stage smoke first; ordinary timeout120 s, initial/full expensive budget up to600 s explicitly approved/recorded, stage budget1,200 s. Existing runner normal calls may still enforce120 s even if initial timeout600; do not assume --initial-timeout raises full-index timeout. Stop large scale when setup fails; retain censored receipt, no automatic retries.
10. Two conditioning +20 cheap successful samples; 3 expensive samples initially; tail claims only p95≥100 and p99≥1,000 successes. At historical 40 s check, 2+3 calls cost roughly200 s; a historical full-index sample about207 s; three plus copies/builds can consume a stage. Budget separate campaigns, not one giant cross-product.
11. Sample only representative slow/growth cases with pinned release symbols; three paired ordinary/profiled attempts where feasible, separate profiles/overhead. Missing permissions, zero/truncated samples, unsymbolized stacks remain explicit gaps. No installation or global security changes. Very short help/config/init calls primarily need repeated timing/CPU measurements; do not fabricate useful 1kHz profile attribution.
12. Record wall/CPU/RSS, setup cost, corpus bytes, DB/WAL sizes, knowledge/source/edge counts, unique search hashes, vector coverage/dimensions/backlog/failures, output bytes, process scope and request counts. Validate independent IDs/flags/diagnostic counts and inspected bytes. Label host contention/page cache and server resource gaps; never equate fallback success with hybrid participation.

### Gaps

- No current root test compile result: nothing compiled or executed. Toolchain unavailability/offline dependency gaps remain preflight blockers, not reasons to silently fetch/install under this read-only task.
- Existing native-Ollama fixture cannot satisfy OpenAI-only enabled campaign unchanged. This is a concrete compatibility gap, not authorization to create new scripts now.
- Current permissions, actual reset/storage costs and graph-stress limits require future approved smoke runs. Historical figures cannot justify a comprehensive completion estimate or SLO claim.

### Read-only review addendum — 2026-11-17

Execution is now approved elsewhere with Samply and deterministic localhost embeddings; harness implementation belongs to another owner. No build/benchmark/script execution or tooling changes during this review. Compilation must finish before measurement. See [coverage_review.md](coverage_review.md) for prioritized source-verified case IDs.

- Existing report validator is clean-baseline-specific: new fault/empty/busy/batch cases require independent scenario expectations; index oracle needs exact counters/revision behavior beyond positive revision. [Source](../../tests/performance/report.py).
- Source-only edits can yield source_changed without changed cache revision/search_hash/backlog. Missing prior ranges can combine source_changed+source_not_found; changed bytes alone need not produce check finding. Valid Markdown edits normally refresh; invalid edited last-valid owners provide deterministic stale case. [Exact tests](../../tests/provenance_commands.rs).
- Empty/punctuation query exits before query inference but after refresh/config/requested-revision validation. Query becomes alphanumeric tokens joined AND; channels each bound by clamped L<=100 before eligibility, without refill. [Source](../../src/service/search.rs).
- Edge_limit caps returned edges, not exploration; unknown traversal starts reject even depth0. Busy drain readiness precedes embed lock and may reset identity; exit0 also includes unavailable/already-running/config-changed, not just complete coverage. [Graph](../../src/service/graph.rs), [drain](../../src/service/embedding.rs).
- OpenAI loopback mock needs nonempty synthetic OPENAI_API_KEY and indexed /v1/embeddings responses. Existing performance setup/fake is disabled/native Ollama; missing-key failure is not HTTP workload. [Actual client](../../src/embedding/provider.rs), [existing fake](../../tests/performance/provider.py).
- R-6 timing is historical SQL storage probe without full CLI/provenance/inference/fusion. R-8 no-index assumptions are historical rather than current flat/cos configuration. [R-6](../../docs/zkit/letopis/research/R-6-sqlite-capacity-probe.md), [R-8](../../docs/zkit/letopis/research/R-8-durability-and-search-primitives.md).
