# Search optimization experiments and focused integration

The user requested optimizing the measured search cost and explicitly authorized
research/experiments in different subagents. Baseline is captured product
`3d820de41ee12385bc734ac4ceeb56b78ba379f9`: full synthetic 100k knowledge,
100k sources, 500k edges and 100k real embeddinggemma vectors (768 dimensions).
Mixed search median is 111.853 s; native CPU eligibility 96.91%, Markdown parsing
53.64%, historical hashing 20.02%, live source hashing 8.40%, vector lookup 1.90%.
These inclusive shares overlap. Historical reports remain unchanged.

## Invariants

Keep every current live-file/source observation and its demand order. Reuse only
pure derived work after fresh input identity has been checked. Preserve full
eligible-channel reranking before fusion/limit, filters, ties, requested-root
validation before inference/exits, stale/missing/invalid behavior, first demanded
infrastructure failure, pinned read views and configuration identity. No new
runtime, dependency, public configuration, schema or persistent payload cache.
Do not reset the populated vector database or rerun the document drain.

## Independent experiments

1. Last-file operation-local eligibility memo after a fresh file capture and
   current byte hash on every candidate. Reuse the existing current-knowledge
   predicate, retain successful derived data only, release edges/search text not
   used by that predicate. Bound retained input/ID bytes and element count; bypass
   oversized inputs without rejecting valid files. New search invocation starts
   empty. Prove changes, invalid/missing states, path separation, error order and
   operation lifetime. Candidate source owner: service/search.rs; avoid a new API.
2. Intrinsic Markdown parsing: replace repeated prefix line counting with a
   monotonic byte/line cursor over validated disjoint element ranges. Preserve
   exact ParsedFile, Unicode/CRLF/EOF coordinates, diagnostics and validation order.
   No lexer replacement or additional retained payloads. Owner: markdown/scan.rs.
3. SQL/read amplification: measure indexed metadata, revision and edge queries
   before adding reuse or interfaces. Reject low-effect/contract-sensitive changes.
4. Ranking-first/native-K truncation: prove compatibility or reject with literal
   counterexamples and existing beyond-limit failure tests. Existing contracts
   require observation/error/capture order and complete eligible channels.

Each agent owns an ignored source clone and Cargo target only. Preserve the main
checkout and the unrelated issue-97 worktree. Different CPU-heavy experiments are
not ordinary comparison evidence. The coordinator serializes full100k screening
and final alternating before/after calls after builds/tests have stopped.

## Integration and verification

Select only measured, contract-preserving improvements. A combination is permitted
when independent changes remain simpler than another abstraction; its gain must
be measured rather than adding individual percentages. Keep source/binary hashes,
patches, raw failures, work counts, exact request/result oracles and new Samply
stacks in a new evidence directory. Do not relabel earlier partial-vector queries
as the full-coverage baseline.

Meaningful tests use Given/When/Then and manual mutations in isolated targets.
Run relevant suites for each candidate and make verify on the final changed root.
Perform independent final code/data review, local Markdown link/hash checks and
source identity verification. Leave intended changes uncommitted in the requested
checkout; do not push, merge, publish issues or clean unrelated state.
