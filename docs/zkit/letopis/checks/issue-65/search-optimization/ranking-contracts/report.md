# Search ranking and observation constraints

Read-only research, 2026-11-06. Product files were not changed; no build, product
test suite or timing was run. References below address the current workspace.

## Verdict

**No general ranking-first shortcut that skips candidate observation preserves
today's contract.** Computing preliminary ranks earlier is harmless if it does
not change the demanded operations, final ranks, errors or fresh captures; it
does not remove the main observation cost. Exact top-k mathematics alone is
insufficient: success currently certifies the absence of demanded infrastructure
errors across the complete candidate set, including candidates beyond `limit`.

Two separate obligations apply:

1. X-1-cli.md:58–70 requires complete eligible-channel reranking before fusion
   and final limit; eligibility is not postfiltering of fused raw ranks.
2. The approved issue-67 spec:143–163 preserves fresh observations and Search's
   interleaved demand/error/capture order. `search.rs:143–163` visits candidates
   in identity order: bindings, source/revision matching, live knowledge,
   provenance, then flags. All metadata integrity checks already ran at
   `search.rs:312–359`. First error aborts, without partial links.

These references mean `docs/zkit/letopis/contracts/X-1-cli.md` and
`docs/superpowers/specs/2026-10-30-issue-67-product-git-optimization-design.md`.

## Minimal counterexamples and existing evidence

* **Fuse raw ranks then filter:** text order `[excluded,a,b]`, vector `[b,a]`,
  eligible `{a,b}`, limit 1. Required scores are a=b=3/2, so identity chooses a.
  Raw ranks produce a=1, b=4/3, incorrectly choosing b.
  `src/service/search.rs:1847–1862`
  (`eligible_channel_ranks_are_recomputed_before_fusion`).
* **Cap native K at output limit:** text `[z,a]`, vector `[y,a]`, limit 1.
  Complete channels give a=y=z=1, hence a wins by identity. Vector K=1 removes
  a's overlap and chooses y. This is the literal case in
  `search.rs:907–915` (`reciprocal_ranks_reward_overlap_before_applying_small_limit`).
  With vector-only `[excluded,a]` and eligible `{a}`, K=1 instead returns nothing;
  see `excluded_channel_heads_do_not_consume_limit`, `search.rs:1866–1878`.
  Any fixed overfetch can have that many excluded predecessors. Current native K
  is vector-key count (`src/storage/vector.rs:634–675`), before joining current
  nodes by search_hash; orphan hashes can also consume a capped prefix.
* **Validate only winners:** a wins, b is below limit 1, but b has malformed
  persisted URI `../wrong.md`. Required answer is internal_error, not `[a]`:
  `tests/knowledge_search.rs:310–335`, explicitly named
  `malformed_persisted_knowledge_uri_fails_even_beyond_final_limit`.
  Discarded NaN metrics likewise still fail: `search.rs:1909–1924`.
* **Observe in rank order / precompute all revision comparisons:** a is first by
  identity, b first by score; a's observation fails. Required events end at
  `[validate,compare,observe]`; no b comparison is allowed.
  `first_observation_failure_precedes_later_candidate_comparison`,
  `search.rs:1277–1289`, plus approved spec:155–163.
* **Reuse a path's live result:** two observations of the same source see
  `basis\n`, then `edited\n`. Required flags are empty, then source_changed;
  `src/service/provenance.rs:344–364`. An unavailable basis becoming available
  must be retried (`search.rs:695–718`; provenance.rs:462–482).
  Observed flags drive both eligibility and rendering exactly once
  (`search.rs:2007–2030`), so a second winner-only observation is also unsafe.
* **Fast exit before preflight:** empty query plus missing requested tree must
  fail NotFound with zero configuration/provider calls:
  `search.rs:1928–1976`; `src/service/mod.rs:461–470` mandates ordered root
  preflight independently of candidates.

Other invariants: pinned metadata/bindings/basis/revision survive concurrent
publication (`search.rs:1480–1509`); non-ready configuration keeps FTS and one
read view (`1407–1424`). Missing sources remain discoverable with flags and
recheck filters (`tests/search_filters.rs:151–196`;
`docs/zkit/letopis/contracts/I-4-sources.md:66–94,127–129`). Identity ties,
deduplication and signed-zero equality are explicit at `search.rs:463–485,
841–885,1882–1904`.

## Viable directions and safety conditions

These are proposals, not accepted implementation decisions:

* **Make every demanded observation cheaper.** Improve parser internals while
  preserving complete-file validity, element hash/ID/coordinates, failure
  classification, fresh byte captures and candidate order. A known-content fast
  path must compare the pinned element metadata too: equal file bytes alone do
  not detect corrupted cached coordinates (`knowledge_search.rs:338–363`;
  current predicate `src/service/utils.rs:235–242`).
* **Bounded reuse of pure parse results** may eliminate repeated parsing of
  identical freshly captured bytes for the same path. It must still perform each
  fresh confined file read at its existing demand point, distinguish changed
  bytes, retain per-candidate metadata checks, bound memory, and avoid reusing
  unavailable/I/O observations. The issue-67 spec permits reuse only inside
  appropriate observation boundaries; extending a boundary needs explicit review.
* **Preserve existing cheap source exclusions.** Exact source membership can
  already reject a candidate before live parsing/provenance (`search.rs:144–147,
  189–230`). Reordering other fallible checks to create more exclusions is unsafe.
* **Exact adaptive channel retrieval** would require complete eligible-rank,
  overlap and tie bounds, a pinned identity/view, and a separate complete
  integrity/observation sweep preserving first-demand order. Thus it may save
  native/materialization work, but cannot generally skip live validation under
  today's contract. A top-results-only error guarantee would be a requirements
  change, not an optimization justified by current tests.

`python3 -B probe.py` exercises three independent literal ranking cases without
product imports. Expected winners are literals, not derived through the shortcut.
Result: all three cases passed. An in-memory mutation removing accumulated
cross-channel weight failed the first literal assertion; no file mutation remained.
