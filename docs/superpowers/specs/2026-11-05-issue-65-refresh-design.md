# Issue 65: current-revision performance follow-up

The user requested implementation, verification, profiling and an attempt to
resolve measured bottlenecks on 2026-11-05. The original issue-65 runner and
diagnosis are already present; issue 67 subsequently replaced product Git
subprocesses. Historical measurements remain historical.

Use the existing model-free runner against commit `52ed491`, with separately
pinned release binaries and new evidence directories. Correct the obsolete
`reindex` invocation to `index --full`, retaining the historical scenario name
for comparison files. Test the request, writer and publication oracle changes.

Measure the existing scenario matrix at 1k/10k/100k within bounded budgets.
Separate first publication, conditioning and ordinary fresh-process calls.
Preserve correctness oracles, failure receipts and explicit unavailable rows.
Real providers, paid calls and formal gates remain outside this work.

Attempt samply and native wall sampling. If host permissions prevent attachment,
retain those failures and build a separate source-identical copy with temporary
operation timers. Archive its exact source changes and binary identity; measure
its overhead separately. Inclusive function durations overlap and cannot be
summed. No diagnostic module ships in the product.

Only optimize a confirmed cost when a focused change preserves snapshot,
durability, freshness and memory contracts. Test first, mutation-check new tests,
and measure the ordinary before/after releases on equivalent input states.
Record unresolved costs and platform limitations without claiming speedups.

The isolated timer dump confirmed repeated compression/staging/publication during
inspect. The focused candidate reuses only an existing loose object after checking
its type and collision-aware content identity, syncing its backing file and parent
directories, and checking command cancellation before and after confirmation.
Missing/packed-only objects retain the original staged publication protocol.
It introduces no persistent cache, new dependency or public service interface.
The existing pathname/replacement limitations of duplicate publication remain;
this change makes no stronger filesystem-race or power-loss claim.

The main checkout's Git metadata is read-only in this session. Leave intended
changes reviewable in the working tree; temporary source copies and synthetic
repositories are isolated under ignored evidence directories. Do not push,
merge, publish issues or alter historical evidence.

## 2026-11-06 native profiling continuation

The user explicitly requested genuine Samply recordings followed by a focused
optimization. The current unrestricted executor records both 100k commands
successfully; prior permission failures remain historical evidence.

Implementation decision: memoize successful tree-body structural validation in
the invocation-owned raw reader, bounded to 4096 OIDs. Every request still reads,
decodes and independently verifies the current object's exact type and content
identity before consulting this metadata. No tree/blob payload cache is added.
Failed validations and unavailable objects are not cached; overflow validates
normally. Separate readers start empty. Full closure audits still validate every
visited tree body directly, without consulting this memo. No public interface,
composition, filesystem policy, live-source capture or error ordering changes.

The larger historical-file-byte cache is deferred because different source
ranges currently demand fresh raw Git verification; sharing their earlier bytes
could hide corruption between those observations. Keeping fresh raw reads trades
away a larger hypothetical speedup to preserve the existing guarantee.

Tests must prove validation reuse and bounded fallback, fresh-reader isolation,
rejection of corrupt/wrong-type/missing replacements after success, repeated
invalid-body rejection, recovery after a failure and uncached full audits for
both SHA formats. Before/after release measurements and new Samply recordings
must use pinned input hashes and valid command/count oracles. Real Ollama work
uses a separate copy of the synthetic 100k corpus and records actual vector
coverage independently; model/OS caches and shared-host interference are explicit.
