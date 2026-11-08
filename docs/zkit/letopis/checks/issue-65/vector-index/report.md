# Exact SQLite vector index

2026-11-07. The user requested implementing SQLite vector-index acceleration and
recording findings in Letopis knowledge. The implemented choice is exact
`flat/cos` in the existing pinned vec1 0.8. No ANN, candidate cap, new provider,
runtime, dependency or embedding identity was introduced.

Six reviewed files are integrated and actual-root host verification passed.
Native SQL improved substantially; **whole-query wall latency did not show an
improvement** in the final three pairs. All six ordinary outputs are identical.

## Complete ordinary query

Same copied synthetic100k corpus,100k real768-dimensional vectors,100k source
nodes,500k edges, query `case 000000`, final limit5. Exact complete candidate
channels and all dynamic eligibility checks remain enabled.

| Measurement | Before | After |
| --- | ---: | ---: |
| Ordinary wall samples, s |75.465,65.210,65.593|65.830,66.986,62.756|
| Wall median, n=3, s |65.592816|65.829875|
| Median CLI CPU, s |61.260561|60.878424|
| Maximum measured CLI peak RSS, MiB |187.9375|158.921875|

The wall median is descriptively0.36% higher; this small shared-host series does
not establish an end-to-end acceleration. No sample was removed or pooled with
earlier campaigns. Per-pair order is before/after, after/before, before/after.
All six responses have the same SHA-256
`0049eca6e5b11edf62ed5bf46ba40b1f89be69d48b3b408856ccf56589da85a2`,
including the five expected literal IDs, their order/paths/coordinates/revision
and clean flags. Per-call state/binary/source checks passed.
[Summary](comparison-summary.json), [campaign pins](comparison-manifest.json),
[exact commands](commands.md), [driver-method review](review/data-method-review.md),
[independent final audit](review/final-data-review.md).

First actual-product `init` on the populated none/cos copy took3.435155 s,
including its checked physical conversion and ordinary initialization. It
preserved100000 native vectors/keys, dimensions768 and all307200000 original
payload bytes. This is a separate one-time observation, not an ordinary search
sample. An initial SHA3 comparator used a differently spelled SELECT; sha3_query
digests differ with SQL text. The exact baseline SELECT rerun matched the original
digest. That failed comparator is retained separately and is not a product fault.

## Native experiment

Owned online backups of the existing synthetic corpus retain100000 actual
embeddinggemma vectors of768 dimensions. Query is the stored embedding at rowid1;
this diagnostic is not the product query-provider path. The SQLite CLI is3.54.1;
the product pins bundled3.53.2 and the same vendored vec1 source.

| Native SQL CPU median, n=3 | none/cos | flat/cos |
| --- | ---: | ---: |
| K=5, seconds |0.087641|0.054451|
| K=100, seconds |0.088012|0.057254|
| K=1000, seconds |0.133266|0.056556|
| K=100000, seconds |2.145223|0.107106|

Full-K native speedup is20.03×; native wall medians are2.402859 →0.135671 s.
These include result serialization and exclude CLI refresh, eligibility,
provenance, filters, fusion and inference. They do not establish whole-search
latency. All24 outputs match the full-result prefixes in rowids, order and
17-digit distances. Ordered SHA3-256 over all100000 vectors matches exactly,
covering307200000 original payload bytes. Native rebuild took2.174718 s wall
and1.230616 s CPU. Original corpus/model/configuration was not modified.

[Native report](native-probe/report.md), [summary](native-probe/summary.json),
[archive scope and verifier](native-probe/README.md). The compressed archive maps
each stored file back to its original uncompressed bytes/hash; failed early
timer-capture attempts remain distinct from the24 accepted calls.

Flat insertion replaces a small block at commit; the reuse threshold defaults
to16KiB. It does not rewrite an existing approximately1MiB rebuilt block for each
singleton append. Fresh singleton-built layout nevertheless differs from the
bulk rebuilt layout tested here. Physical page/WAL cost and a fresh100k drain
after flat were not measured; the earlier142-minute model-drain result is not
transferred to this implementation. [Source-only note](native-probe/write-path-note.md).

## Product behavior and safety

- New databases and configuration resets create flat/cos. Recognized existing
  none/cos databases are prepared once during writable bootstrap before consumer
  attachments. Already-flat preparation performs no persistent writes/rebuild.
- Mode classification compares opaque model bytes with factory models produced
  by the same pinned native module. Public vec1cat descriptors normalize unknown
  distance tags, so production does not trust them or parse a private header.
- The existing bounded contention policy covers preflight and immediate writer
  acquisition; mode is rechecked after waiting. Positive integer native MODEL
  version with increment room, dimensions, byte widths and finite original
  components are checked before native rebuild. Failure rolls back; no identity
  reset/reembedding or corruption repair is substituted.
- Original rowids, vector bytes, search hashes, dimensions, embedding identity,
  attempts and diagnostics survive. Flat blocks are authoritative vector storage,
  not a disposable secondary index. Query-only legacy and read-only status retain
  compatible observation paths. Logical vector schema version remains1.
- Full vector/FTS channels, eligible-channel reranking/RRF/ties, source filters,
  fresh knowledge/source observations and candidate errors beyond final limit
  remain in their existing owners; search orchestration was not changed.

The implementation has one private physical-index preparation module,
`src/storage/vector_index.rs`, invoked by schema bootstrap. `VectorStorage` and
its trait implementations remain in vector.rs. Only decoder visibility widens
to sibling scope with its documented existing contract. No application trait,
service, domain or composition API changed; the current DDL mirror is updated.

[Design](../../../../../superpowers/specs/2026-11-07-exact-vector-index-design.md),
[decision D-107](../../../decisions.md#D-107),
[independent review](review/report.md), [candidate](candidate/report.md).

## Corruption and regression evidence

Native controls show malformed length rejects rebuild and retains none. NaN/Inf
can survive native rebuild; both modes expose NULL distances. An early printf
control misleadingly rendered NULL as0.0; corrected typed controls are retained.
The product retains checked metric decoding and validates original finite bytes
before legacy conversion. This is not a general native corruption audit.

Ten Given/When/Then scenarios cover populated-state/idempotence, separate pinned
readers, malformed/nonfinite data, query-only legacy, failed native rebuild and
same-connection retry, competing writers, fresh/empty retained dimensions,
unsupported metrics, unknown distance code3 hidden by vec1cat, and corrupt native
MODEL version. The latter two defects were found/fixed during independent review;
genuine RED/GREEN evidence is retained. Six viable production mutants were caught
by behavioral failures, and all frozen source hashes were restored afterward.

Candidate `make check` passed1217 root debug tests,11 ignored, plus9 lint-example
tests; format/all-target check/Clippy passed. Actual-root host storage runner
then passed all16 commands, including `make verify V=1`, schema parity, runner
tests, the explicit real60-second provider deadline, strict Rustdoc, native
capability probing and copied standalone executable/linkage checks. Its nested
make-verify log has171 successful result groups,2443 passed test executions,
zero failed and22 ignored executions across profiles/examples; these are not
unique-test counts. Patch and six-source identities are
[pinned](candidate/source-manifest.json); measurement capture in the isolated
clone is`016eae8fa9705a209b01555a32877702b70cf74a`. Root remains uncommitted.
[Actual-root verification](verification.json),
[host runner manifest](host-storage/manifest.json),
[complete make-verify log](host-storage/rust-check.log).

Final immutable product SHA-256:
`ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9`.
All107 product inputs, including the new module, match the reviewed capture and
actual root. Baseline is the previous captured2db40fd product, SHA-256
`bd2c79b6b3dff6858fd021ca7af521f9ceff735279844d76638f1879fa9fb0b6`.

## Genuine Samply

Recorded separately after ordinary timing, save-only1000Hz with local symbols;
no server/browser/upload.79244 usable samples from two threads give70303137
microseconds of observed CPU. Product Mach-O UUID
`72CF737B-8ADA-393A-AC42-37E4C4EF088C` identifies the recorded release.
[Recording](profile/search.json.gz), [symbols](profile/search.json.syms.json),
[validated samples](profile/validation.json), [inclusive stacks](profile/stacks.json).

| Inclusive observed CPU share | Current flat profile |
| --- | ---: |
| Complete candidate eligibility |96.70%|
| Historical source hashing |35.83%|
| Live source hashing |16.17%|
| Current knowledge capture/hash/matching |34.76%|
| Native vector query, join and decode owner |1.00%|
| Markdown parser |1.12%|

Scopes overlap and must not be added. These sampled CPU shares are not wall
percentages or ordinary resources. The profile is a separate host context and
is not used to estimate wrapper overhead. Candidate observations and provenance
continue to dominate full search; accelerating native retrieval alone does not
remove those checks. No freshness/first-demand/complete-ranking shortcut was used.

## Knowledge publication

Findings were recorded in the existing `letopis/architecture/storage.md` owner,
preserving ID/created_at and sources. Root knowledge cache is schema2 and works
with installed Letopis0.2.0; the current development binary expects schema4.
The compatible installed binary was used rather than resetting the working cache,
changing provider/configuration or installing a new global binary.

Scoped check succeeded, initial filtered search returned revision96 with no flags,
and postintegration inspect/readback verified the updated knowledge and19 sources
from the same captured tree. Initial and postintegration receipts are preserved in
[knowledge](knowledge/report.md) and [postintegration](knowledge/postintegration/report.md).
Whole-command measurements are not inferred from native results in the knowledge.
The latest publication is revision100, with27 verified source bindings and inspect
snapshot`492ac2d345d9a835d30c613c8d0c09b2e347958d`.
The final named correction also removed an older introductory claim that edge
targets lack foreign keys; current schema uses both endpoint FKs and
`unresolved_edges.target_uri` for missing targets.
[Final knowledge receipt](knowledge/final/report.md),
[intro correction and readback](knowledge/final-correction/report.md).
Successful vectorization of
the updated knowledge element is not claimed; verified publication/check/inspect
does not require changing its existing provider configuration.

## Measurement conditions and limits

Whole-CLI comparison uses one separately copied fixture with both databases made
through SQLite online backup. All4334 canonical source hashes matched. Initial
139.045 s conditioning included copied-file refresh and concurrent untimed setup;
it is excluded from comparisons. Physical-mode rebuilds, query warmup and help
startup probes are outside ordinary timers. First production migration is measured
separately. The original fixture and earlier evidence remain unchanged.

The host is shared and OS cache uncontrolled. CLI wait4 resources exclude Ollama;
they are not whole-stack simultaneous memory. No formal minimum-hardware, recall,
production SLO, tail-latency or Linux/x86 verdict follows from this Mac experiment.
Existing contracts require complete exact ranking, so ANN is not enabled merely
to produce a smaller candidate set. Exact modes still scan all vectors; most
remaining end-to-end work is outside native distance retrieval.

[Preservation audit](preservation.json) rechecked107 actual product inputs,
269 host-runner inputs,4334 original and4334 copied corpus files, and all486
prior indexed evidence artifacts. The original100k vector payload also retains
its exact ordered digest. Existing unrelated tracked diffs and root HEAD were
preserved. [New evidence index](evidence-index.json) pins this archive separately.
