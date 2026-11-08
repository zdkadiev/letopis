# Search optimization: competing methods and measured result

2026-11-06. The user requested optimizing search and trying different approaches
through independent research/experiment agents. Four approaches were investigated;
two compatible changes were integrated. Hybrid search on a synthetic 100k corpus
with real-provider vectors had its median fall
**111.886 → 61.102 s**:
**45.39% lower latency, 1.831× speedup**.
All six matched calls and ten separate narrow-FTS controls passed their oracles.

## Methods evaluated

| Method | Evidence | Decision |
| --- | --- | --- |
| Intrinsic Markdown line-coordinate cursor | Parser microbench1.28230 →0.81233 s per2000 parses; literal line-count visits1799970 →59999 bytes; full screening 91.075 s | Retained: two local cursor values, no new allocation/API |
| Pure last-file parse reuse after fresh capture/hash | Scoped test retains16 fresh captures but reduces parsing16 →2 over two operations; full screening 62.733 s | Retained: one bounded successful projection |
| Both changes | Full screening 61.146 s | Selected and validated below |
| SQL/point-read reduction | Indexed lookups, zero scan steps; prepared-query probe saves about1.089 SQL-only seconds; revision owner0.237 sampled CPU seconds | No product SQL change: small effect, demand-order constraints |
| Ranking-first or native-K truncation | Literal counterexamples and existing beyond-limit malformed-candidate error test | Rejected: changes eligible-channel fusion/error/observation contract |

The screening control was 117.020 s. These are
**one screening sample per variant**, not comparative medians. Parser microbench
and work-count reductions are not whole-search speedups; individual gains are
not added. [Parser report](parser-cpu/report.md), [memo report](parse-memo/report.md),
[SQL report](sql-cost/report.md), [ranking constraints](ranking-contracts/report.md),
[screening summary](screening-summary.json).

## Final ordinary comparison

Fixture:100000 knowledge nodes,100000 source nodes,500000 edges,3334 marked files,
1000 source files and100000 native768-dimensional embeddinggemma vectors. The
existing completed corpus was reused; no document reembedding or identity reset.
Model:`embeddinggemma:300m-qat-q4_0`, query:`case 000000`, limit5. Before/after
use the same unchanged corpus, with fresh CLI processes and pinned releases.

| Measurement | Before | After |
| --- | ---: | ---: |
| Full hybrid samples, s | 114.434, 110.918, 111.886 | 62.916, 60.997, 61.102 |
| Full hybrid median, n=3, s | 111.886 | 61.102 |
| Median CLI CPU, s | 106.628 | 57.269 |
| Maximum CLI RSS, MiB | 186.95 | 180.64 |
| Narrow FTS median, n=5, s | 0.682530 | 0.674626 |

Three hybrid pairs alternate before/after, after/before, before/after. Five narrow
FTS pairs are separate. Narrow FTS has one eligible lexical hit and shows no
material improvement established by this small series. It is not pooled with
hybrid. Every call preserves the independently expected literal hit; hybrid
additionally requires four other known current URIs with clean flags. Counts and
vector state remain stable. Output membership establishes vector contribution
and lexical-hit retention; it alone does not establish the causal contribution
of the lexical branch. Retrieval-path execution is separately visible in native
profiles and unchanged search control flow.

A query-only model warmup and executable --help startup probes are outside timers.
OS page cache is uncontrolled. No owned build/test/profile/drain was intentionally
run during final ordinary timing; the host remains shared. CLI wait4 CPU/RSS
exclude the Ollama daemon/model and are not whole-stack or simultaneous memory.
Logical memo budgets are not allocator/RSS caps. n=3/n=5 support no p95/p99 or
formal minimum-hardware/production acceptance verdict.

[Comparison summary](comparison-summary.json), [manifest](comparison-manifest.json),
[resources](resource-summary.json), [FTS controls](fts-summary.json). Per-call raw
and validated records, manifests and startup receipts are in pairs/ and fts/.

## Implementation and preserved behavior

`markdown/scan.rs` computes source lines incrementally after each already-validated
disjoint element. The cursor stores the line at byte offset end-1, counts the gap
to the next begin, then the next inclusive span. Raw byte slices preserve Unicode,
CRLF and no-final-LF behavior. Complete parser projections and diagnostics remain
equivalent; no lexer replacement or validation reordering.

`service/search.rs` owns one CurrentKnowledge state inside eligibility. Each
binding first captures the live file through the existing confined FileReader
and independently hashes its bytes. A successful parse is reused only for the
same path/hash; the existing centralized id/hash/line predicate still checks each
pinned node. Edges/search text are released before retention. Retention bypasses
inputs>256KiB,>1024 elements or>256KiB combined path/ID content. The logical
retained envelope is about496KiB plus holder/allocator overhead; it is not an RSS
ceiling. Missing/invalid/unmarked observations are not reused; a new operation
starts empty. No raw Markdown bytes are retained by this state.

Fresh live/source captures, provenance, complete metadata integrity checks,
requested-root preflight, first-demand error order, filters, deterministic ties,
complete eligible-channel ranking and pinned storage/configuration remain.
No native K cap, schema, provider, dependency, runtime or public option was added.
Current MarkdownCodec docs already guarantee deterministic/no-I/O results for the
same path/bytes; no new contract was assumed silently.

## Samply confirmation and remaining costs

The new genuine Samply0.13.1 recording contains 65104 usable
stacks, with save-only1000Hz sampling and local presymbolication. It was recorded
after ordinary timing and is not an extra ordinary latency sample.

| Inclusive observed CPU share | Earlier full baseline profile | New profile |
| --- | ---: | ---: |
| Markdown file_parse |53.64%|1.19%|
| layout_scan |31.69%|0.39%|
| Historical source hashing |20.02%|35.17%|
| Live source hashing |8.40%|14.57%|
| Native vector retrieval |1.90%|4.47%|

Scopes overlap and cannot be added. Profile host conditions differ; their sampled
CPU totals are not the matched ordinary comparison. Rising relative shares of
remaining work do not imply a corresponding absolute regression. Fresh knowledge
capture/hash/current matching in CurrentKnowledge::build_link accounts for
34.67% of the new observed CPU, including
its nested work. Historical reads and current knowledge/source captures/hashing
remain the next costs; they still preserve current freshness/corruption guarantees.
[Raw recording](profile/search.json.gz), [symbols](profile/search.json.syms.json),
[stacks](profile/stacks.json), [validation](profile/validation.json).

## Verification, isolation and provenance

- Final root `CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 make verify V=1`
  passed: format, all-target check, Clippy, fixed-value lint, root debug/release
  suites and strict Rustdoc. 2423 successful test executions and22 ignored executions
  include profile/example repeats; this is not a unique-test count.
  [Receipt](verification.json), [full log](root-verify.log).
- Memo tests use Given/When/Then and cover fresh captures, byte edits despite equal
  stat values, invalid/missing/restored files, path/operation isolation, per-node
  hash/line checks, all retention budgets, released payloads and I/O retry. Ten
  viable production mutants were caught on final dyn source; restoration matched
  source hashes and final eight cases passed. Initial dead-code-unviable mutation
  formulations are retained separately, not counted as detected behavior mutants.
- Parser coordinate test has independent literal lines with gaps, CRLF, fenced
  headings, Unicode and EOF. A reset-cursor mutation failed. Initial UTF-8 str-slice
  failure was corrected to raw byte counting, then tests and full verify passed.
- Both agents independently passed full make verify; combined36 search unit tests
  and related Markdown/search integration suites passed before actual-root verify.
  One first coordinator command named a nonexistent markdown_lifecycle target;
  it ran no tests. Its log is preserved, corrected invocation passed.
- The successful116.616 s conditioning response was independently revalidated
  after a wrapper attempted to create its JSON twice through an exclusive writer.
  Raw/final receipts now have distinct names. This was a measurement-harness
  error, not a product failure. Failed wrapper output remains in screening.log.
- [Independent code review](review/report.md) found no actionable findings.
  [Independent data audit](review/data-review.md) recomputed ordinary results,
  source/corpus identities, verification counts and all native function totals.
  It found no material discrepancy; its minor synthetic-corpus wording correction
  is reflected above.
- Before source snapshot:`3d820de41ee12385bc734ac4ceeb56b78ba379f9`; copied binary
  `9a3e2b70a907d135604ad04e6491e358d07575e987c1457d514a1820c2d2ba6a`.
  After source snapshot:`2db40fd82fd8b504ebb2040b96b248031ce5d791`; copied binary
  `bd2c79b6b3dff6858fd021ca7af521f9ceff735279844d76638f1879fa9fb0b6`. All106 actual root product inputs match the after capture;
  root HEAD itself remains unchanged with uncommitted intended changes.
  [Before](baseline-build.json), [after](after-build.json).

Agents used separate ignored source/target directories; final timing was serialized.
The unrelated issue-97 worktree and prior user edits were preserved. Historical
reports/evidence were not rewritten. No push, merge, issue publication or cleanup
of unrelated work occurred. No Linux/x86_64/native-storage platform suite was run;
product native/storage inputs did not change.

Spec:[search design](../../../../../superpowers/specs/2026-11-06-search-optimization-design.md).
Ledger:[execution plan](../../../../../superpowers/plans/2026-11-06-search-optimization.md).
Exact drivers/requests are archived; use fresh output paths for new experiments.
See [commands](commands.md) for building, querying, validated measurements and
Samply recording. Final corpus and native-vector receipts are
[corpus-verification.json](corpus-verification.json) and
[native-vectors-final.json](native-vectors-final.json).
The [final consistency audit](final-consistency.json) rechecks 106 product inputs,
4334 corpus files, all 284 prior evidence artifacts, verification-log counts,
140 local Markdown links and preservation of the unrelated existing diff.
The [evidence index](evidence-index.json) pins this new archive separately.
