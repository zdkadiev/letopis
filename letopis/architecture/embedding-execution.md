# Embedding execution

## Service and query behavior {#service}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

`EmbeddingService` connects configuration, Markdown-derived search text, storage
and an embedding provider through application interfaces. A missing, invalid,
disabled or changed configuration can make the vector channel unavailable;
full-text search remains available when query embeddings cannot be used.

Query entry permits a pinned cache read but requires an inactive vector owner.
An inherited cache write or any vector scope is rejected before provider calls;
that owner becomes rollback-only even when the parent catches the rejection.
The independent vector writer remains available for query diagnostics.

For a query, the service validates the provider configuration, checks the active
vector configuration and dimensions, calls the provider, then checks the vector
state again before publishing query diagnostics. Search accepts a vector only for
the captured configuration identity and expected finite dimensions.

Document-vector work is handled by drain. Successful foreground operations can
request a detached drain; query inference is part of the search request and is
distinct from this background work.

[Embedding service](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Search vector channel](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")

[Embedding drain](#drain-execution "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#drain-execution")

[Background execution](component-boundaries.md#process-execution "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#process-execution")

## Provider dispatch and HTTP {#providers}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

One `HttpProviders` owns two independent HTTP pools and the captured compatible
credential. It dispatches to private protocol functions in `provider.rs` rather
than separate provider structs: `ollama_vectors_compute` uses the `embed` endpoint
and disables truncation, while `openai_vectors_compute` uses `embeddings` and
bearer authorization. Native requests receive no compatible credential. Both
validate returned vectors against the expected item count and dimensions through
shared HTTP decoding.

[Provider dispatch, protocols and HTTP transport](../../src/embedding/provider.rs "letopis:type=external;path=src/embedding/provider.rs")

## Drain execution {#drain-execution}
<!-- letopis: {"created_at":1793728137,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->

Drain fills missing document embeddings independently of query embedding in
`search`. The synchronous implementation processes one work item at a time.
Drain requires inactive cache and vector owners at entry. Storage rejects and
poisons an inherited scope before configuration or work reservation; fresh reads
on each iteration must observe the vector just published by the previous item.

`embed --drain` can be launched by the foreground runner or invoked directly.
Each drain takes its own `embed.lock` lease before processing work.

Drain observes configuration and reconciles vector identity. Missing, invalid
or disabled configuration makes drain unavailable. An identity change observed
during processing ends the run. A busy lock lets the existing drain continue.

[Drain coordination](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Background launch and process ownership](component-boundaries.md#process-execution "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#process-execution")

[Embedding execution](#service "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#service")

## Work selection and failures {#work-and-failures}
<!-- letopis: {"created_at":1793728137,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->

Pending work is derived from current non-source knowledge nodes: select a
`search_hash` without a stored vector and without an exhausted attempt budget.
Before reserving an attempt, drain reads and parses the current Markdown file and
requires the element ID and `search_hash` to match. Missing or stale elements are
skipped for this run without spending an attempt; successful publication later
makes a shared hash available to all nodes with that hash.

For each selected item, drain commits an attempt reservation in `vectors.db` before
calling the provider. The budget is the initial attempt plus `max_retries`; the
reservation survives interruption. No vector write transaction is held during the
provider request. A valid finite vector with the expected dimensions is stored in
a new transaction and the failure record is removed. Identity is checked again
at write time; a changed configuration stops the old run instead of committing it.

On input, provider or invalid-vector failure, drain records a safe failure for the
hash. If attempts remain it waits one second before work selection resumes; an
exhausted hash is no longer eligible. Storage failures can terminate the command.
When no eligible work remains, drain prunes unreferenced vector keys, vectors and
failure records under the current configuration identity. Status reports backlog
and exhausted failures; successful foreground output does not imply the detached
child has already completed.

[Work selection, pruning, attempts and vector transactions](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

[Drain work loop and retry policy](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")
