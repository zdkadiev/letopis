# Follow-up drafts; not published or accepted implementations

## Historical object decoding after structural-validation reuse

Baseline: captured release `3d820de41ee12385bc734ac4ceeb56b78ba379f9`,
100k knowledge/100k sources/500k edges, embeddings disabled. Ordinary medians:
check 32.038 s, status 31.566 s, three observations each. Native stacks attribute
about 66% of observed CPU to historical hashing, including fresh object reads,
decoding and identity checks. These are overlapping scopes.

Investigate reducing repeated historical work without hiding later corruption,
changing first-demand error ordering, caching live observations or weakening full
closure audits. Historical file-payload reuse remains a hypothesis requiring an
explicit observation-boundary decision. Do not silently cache previously verified
payloads across distinct addresses. Proposed experiment criterion: at least 20%
lower matched median at 100k, unchanged output/count oracles and bounded memory;
this is a candidate criterion, not an accepted requirement or predicted result.

Regression checks: both SHA formats; distinct ranges/trees; corrupt, missing,
wrong-type and restored objects between observations; failures/first error order;
live edits between bindings; oversized payload bypass and finite budgets; strong
pins and packed/delta objects. No new runtime, fetch or process fallback.

## Full candidate eligibility in real-provider search

Baseline: captured pre-memo release `3b2e477cd2bb8bd204ab606a1566f32e713b4ac8`,
100k knowledge entries and partial 20.5k–23k real Ollama vector coverage. A query
with independently verified zero lexical matches returns five valid knowledge
URIs. First query after copying the fixture: 89.680 s; repeat calls: 30.766 and
31.679 s. This is partial-coverage synthetic evidence, not a 100k-vector latency.

Native CPU profile: eligibility 95.33%, Markdown parsing 47.33%, historical
hashing 25.52%, live source hashing 7.81%, native vector retrieval 1.86%. Scopes
overlap and cannot be added. `get_nearest_vectors` requests K equal to stored
vector count; `get_eligible_links_by_metadata_and_filters` validates candidates before final ranking and
limit. Repeatedly parsing whole files and observing provenance is confirmed work,
not evidence that it may safely be skipped.

Use the completed full-coverage baseline below and preserve exact filter/rank/tie,
staleness, live edit, source-revision, malformed-file and first-error semantics.
Naively capping native K or caching live parsed files changes those semantics.
Proposed experiment criterion: at least 2x lower matched median with the same
literal lexical and independent vector-only oracles, bounded memory and unchanged
filter results. Treat the improvement target as a hypothesis. Model inference,
native vector lookup and application validation must remain separately attributed.

## Sequential drain work selection at partial coverage

A three-second macOS `sample` window on the owned resumed pre-memo drain at
roughly 69k populated vectors produced 2569 main-thread observations: 1345 in
HTTP input waiting (52.36%), 954 in the storage next-work query (37.14%). This is
wall-stack observation frequency, not a Samply CPU profile, exact wait accounting
or a whole-run percentage. The rest includes publication and other work; do not
attribute the full duration to the model. Raw stacks and summary are in ollama/.

`drain_ready` resets its cursor after successful work; the next-work query excludes
already completed hashes. Measure query VM steps and CPU versus coverage before
changing it. Any forward cursor/query improvement must still observe concurrently
changed or newly eligible earlier IDs, preserve current-hash publication checks,
attempt budgets, config changes, single-writer leases and completion semantics.
No queue, retry or query change was implemented in this campaign.

One conservative search candidate is reusing only a successful pure Markdown
parse after a fresh current-file capture has matched the same path and content
hash. This differs from caching a live file observation: each binding must still
capture current bytes and preserve edits, missing/unsafe files and demand order.
Use a small operation-local entry/byte budget with oversized-input bypass; retain
no command-long corpus payload set. This is an unimplemented proposal requiring
contract review and mutation-tested changed-between-captures/error cases, not a
promised speedup or a decision to cap native K.

## Completed full-coverage search baseline

The subsequent pinned memo release `3d820de41ee12385bc734ac4ceeb56b78ba379f9`
completed three real mixed-channel searches with all 100000 vectors present.
Median 111.853 s, range 111.007–111.938 s; each result retains the one independent
FTS hit plus four nonlexical known URIs. The full native profile observes 96.91%
CPU in eligibility, 53.64% in Markdown parsing, 20.02% in historical hashing and
1.90% in native vector retrieval (overlapping inclusive scopes). Use this
complete-coverage baseline for future ordinary comparisons. Do not divide it by
the earlier partial-coverage timings and call that a release regression.
