<!-- letopis: {"created_at":1793278527,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->
# Embedding execution

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

[Embedding drain](embedding-drain.md "letopis:type=related_to;path=letopis/architecture/embedding-drain.md")

One `HttpProviders` owns two independent HTTP pools and the captured compatible
credential. It dispatches to private protocol functions in `ollama.rs` and
`openai.rs`, rather than separate provider structs. Ollama uses the `embed` endpoint
and disables truncation; OpenAI uses `embeddings` and bearer authorization. Native
requests receive no compatible credential. Both validate returned vectors against
the expected item count and dimensions through shared HTTP decoding.

[Embedding service](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Search vector channel](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")

[Provider dispatch](../../src/embedding/provider.rs "letopis:type=external;path=src/embedding/provider.rs")

[Ollama protocol](../../src/embedding/ollama.rs "letopis:type=external;path=src/embedding/ollama.rs")

[OpenAI protocol](../../src/embedding/openai.rs "letopis:type=external;path=src/embedding/openai.rs")

[Shared HTTP transport and vector validation](../../src/embedding/http.rs "letopis:type=external;path=src/embedding/http.rs")

[Background execution](process-execution.md "letopis:type=related_to;path=letopis/architecture/process-execution.md")
