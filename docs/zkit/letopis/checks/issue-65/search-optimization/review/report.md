# Independent combined-candidate review

Reviewed 2026-11-06, read-only except this report. Candidate: `combined/source`,
against HEAD `3d820de41ee12385bc734ac4ceeb56b78ba379f9`.

## Findings

- Critical: none.
- Important: none.
- Minor: none requiring correction.

The combined change is suitable for the coordinator's final verification and
serialized full-corpus measurement. This review does not establish its end-to-end
performance benefit or declare final-root verification complete.

## Scope and source identity

Read AGENTS.md; the current search optimization spec and execution ledger;
architecture program-structure/receiver boundaries; X-1 complete eligible-channel
and filter rules; X-4 element/layout/freshness rules; I-4 source/error contracts;
and the issue-67 provenance-reuse/demand-order design. Inspected the whole product
diff, affected implementations and types, existing predicate owner, and associated
new and existing tests. The candidate changes exactly three files:

| File | SHA-256 |
| --- | --- |
| src/service/search.rs | 94a1c8473134b681fa36690368da520bff9fab2d241093775e9fc5d6939f7c41 |
| src/markdown/scan.rs | d1fd2f97f74d08190f57a35f8dd05002bafd80dc1de0d430a917edd1e600b4fd |
| tests/markdown_elements.rs | 13c137df3005475ee6c8a8f49891cf780790179c69124c2e80f787b74841c9de |

`git diff HEAD --check` returned exit 0. No build, test suite, benchmark, source
mutation, Git index/ref update, or root product edit was performed by this reviewer.

## Freshness and error order

`search.rs:157-185` keeps sorted metadata demand, source-binding queries,
same-binding source/revision filtering, project revision query, live knowledge
capture, provenance observation, and flag filtering in the original order.
Only the private receiver performing the existing link construction changes.
`search.rs:246-255` still calls the injected FileReader for every demanded binding,
using the existing missing/error mapping. A hit is possible only after that call
succeeds and `search.rs:279-285` hashes the new captured bytes and compares both
path and SHA-256. Stat fields, cached flags, and prior live observations cannot
substitute for the new capture.

The unchanged `utils.rs:235-242` predicate checks this binding's ID, element hash,
and lines each time, including on a memo hit. Parsed absence/invalidity is not
retained; missing files clear state; changed path/content drops the old parse
before invoking the parser; an I/O error aborts and drops operation state. The
next eligibility call creates an empty memo even if SearchService survives.
There is no current-source/provenance cache and no new eager observation.

The parser's pure determinism assumption is already explicit in baseline
`service/mod.rs:732-734`: every implementation is deterministic, performs no I/O,
and equal paths and bytes give equal results. Verified this with
`git show HEAD:src/service/mod.rs`; no additional trait documentation/API change
is necessary. Suppressing repeated pure parsing therefore suppresses no demanded
infrastructure error or mutable observation. SHA-256 content identity remains
the product's existing hash assumption.

## Retained state and boundaries

`search.rs:291-315` retains at most one successful file projection with input at
most 256 KiB, at most 1024 entries, and at most 256 KiB total path/element-ID
contents. Saturating addition prevents budget overflow. Exceeding a budget only
bypasses reuse and does not reject or truncate valid knowledge. `ParsedFile.edges`
is a BTreeSet, so `clear()` releases its owned edge/node allocations. Assigning
`String::new()` releases each search-text buffer, rather than retaining its old
capacity. Element vector shrinking is requested. Remaining variable payload is
IDs plus bounded optional tree OIDs; coordinates/hashes/metadata are fixed-size.
This is a logical retained-payload/entry bound, not an exact allocator/RSS promise;
ordinary fresh captures and parse temporaries are not newly bounded by this memo.

The new `CurrentKnowledge` receiver owns meaningful operation state and accesses
borrowed `dyn FileReader`/`dyn MarkdownCodec` through its fields. It stays private
in the existing service owner, preserves the shared matching rule, and introduces
no generic propagation, proxy service, public abstraction, adapter orchestration,
domain dependency, wiring, persistent cache, schema, runtime, or dependency change.
Caller/helper order and inline-last tests match the touched-code conventions.
The retained stripped ParsedFile is used only by the existing metadata predicate;
its deliberately discarded search/graph data cannot escape into other consumers.

## Cursor invariant and parser behavior

`scan.rs:121-137` preserves metadata/layout/ID validation order and rejects overlap
before consuming the cursor. Let N(a,b) count LF bytes in [a,b). The cursor starts
with offset 0 and line 1, maintaining line_number = 1 + N(0,line_offset).
Non-overlap guarantees the next begin is after the prior end-1. Adding
N(line_offset,begin) yields exactly the previous prefix-count start coordinate;
adding N(begin,end-1) yields the previous inclusive-end coordinate. Saving end-1
reestablishes the invariant. It excludes a final LF from its own last line and
includes it when moving to the next section. Valid ranges are nonempty because
each includes its metadata declaration. No range can go backward after the
existing layout check.

Both slices are byte slices. Thus an inclusive end inside the final UTF-8 scalar
does not require a str boundary; CRLF still counts only LF, exactly as before.
Whole-file elements, gaps, adjacent sections, EOF without LF, byte spans, element
hashes, and diagnostics keep their original meanings. No event/heading/metadata
collection, link processing, or search-text rendering order changes.

## Ranking, filters, and evidence limits

Search still validates requested roots before configuration/inference/empty exits,
loads full FTS/vector channels and metadata, computes complete eligible links,
then ranks within eligible channels before fusion/limit (`search.rs:94-143`).
No native-K, ranking-first, early-limit, filter, tie, pinned-view, or configuration
identity shortcut appears in the diff. Existing late-error and pinned-view tests
remain applicable; their code is unchanged.

The eight new memo tests use explicit Given/When/Then phases, real parsing of
initial pinned fixtures, scripted captures with unchanged stat fields, and
independent literal flag/count expectations. They cover repeated captures,
operation lifetime, same-size edits, invalid/missing/restored states, path
identity, all three retention budgets, per-binding hash/lines, released payloads,
and late capture error/retry. Parser-derived initial fixture metadata is input
preparation, not the expected outcome oracle. The added Markdown test independently
asserts literal line coordinates 3-9 and 13-15 across CRLF, Unicode, an excluded
fenced heading, a gap, and no final LF. Existing golden/negative tests supply
independent byte/hash/layout/diagnostic coverage.

Read the experiment reports and memo mutation receipt: all ten listed manual memo
mutants were caught; the parser report records a caught cumulative-line-state
mutation and restored GREEN, plus its standalone make-verify pass. These are
experiment-owned evidence, not tests rerun by this reviewer. At review time the
memo report/log still showed its release verification stage progressing, and no
combined/final-root make-verify result or alternating full-search timing was
claimed. Coordinator must finish those already-planned steps against the actual
chosen tree and pin matching source/binary identities. The parser microbenchmark
alone cannot be added to memo savings to infer the combined speedup.
