# Bounded hybrid search

Status: approved by the user and implemented on 2026-11-07. Actual-root host
verification, paired100k measurements and genuine Samply passed; see the
[report](../../zkit/letopis/checks/issue-65/bounded-search/report.md).

## Agreed limit rule

Database search must not return the complete corpus. For a valid positive request
limit, use `effective_limit = min(requested_limit, 100)`; an omitted limit uses
100. Values above 100 are clamped rather than rejected. Zero and invalid types
retain invalid-request behavior. The final response contains at most this limit.

## SQL retrieval boundary

User requirement: both nearest-vector search and FTS execute inside SQLite.
The database performs matching/distance computation, ordering and top-K selection
before rows are decoded into Rust collections. Only selected candidate IDs and
bounded ranking metadata cross the storage interface. Do not load all vectors,
all distances, all FTS matches or all joined knowledge rows into service memory
and truncate there. SQL values, including the effective limit, remain bound.
The SQLite engine and native vec1 naturally execute inside the CLI process;
this requirement bounds application-owned result materialization, not SQLite's
internal working memory or the number of vectors examined by an exact search.
The cap must also hold after search_hash joins that can expand one vector into
multiple knowledge hits. Verify the native top-K and final SQL ordering/limit,
not only a Rust take(). Fusion within the bounded returned candidate pool and
filesystem/Git freshness observations remain service responsibilities.

## Implementation

Use this same effective limit for each retrieval channel: FTS orders knowledge
hits by BM25 then ID, vector retrieval orders them by cosine distance then ID.
Union and deduplicate at most `2 * effective_limit` knowledge hits, observe fresh
knowledge/provenance, apply filters, fuse eligible channel ranks and return at
most `effective_limit`. Thus explicit limit 5 yields at most 5 hits per channel,
not 100. Shared vector hashes must not evade the knowledge-hit cap after joins.

This replaces complete-corpus eligible ranking with bounded candidate ranking;
selective filters may return fewer results. There is no refill outside the bounded
pool. Errors in unread candidates outside that pool are not observed. Per-channel
application is the concrete agreed interpretation of the database limit rule.

## Alternatives and scope

Complete channels followed by checking only winners still transfers the entire
candidate set and does not meet the clarified requirement. A constant 100 per
channel ignores explicit smaller limits. Arbitrary ANN or an async runtime is
unnecessary. Retain the existing native exact flat index and synchronous wiring.

## Verification

Cover omitted, 1, 5, 100, above-100 and invalid limits; tie order; duplicate/shared
vectors; vector fallback; filters; storage row bounds and observed candidate
counts. Test source/knowledge errors inside the pool. Update help, contracts and
current mechanics with the changed guarantees. Measure the 100k corpus at limits
5 and 100, preserve outputs and differences, and record a separate genuine Samply
profile. Run make verify and the native storage runner for storage changes.
The report records measured medians and their local-workload limits; it makes no
production relevance-quality or minimum-hardware acceptance claim.
