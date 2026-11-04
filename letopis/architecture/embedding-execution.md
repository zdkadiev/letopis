<!-- letopis: {"created_at":1793278527,"source_snapshot":"7d506ed54103d557f6da760bd189f084efb74dd5"} -->
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

`HttpProviders` dispatches to Ollama or OpenAI implementations. The Ollama adapter
uses the `embed` endpoint and disables truncation; the OpenAI adapter uses
`embeddings` and supplies bearer authorization. Both validate returned vectors
against the expected item count and dimensions through shared HTTP decoding.

[Embedding service](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Search vector channel](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")

[Provider dispatch](../../src/embedding/provider.rs "letopis:type=external;path=src/embedding/provider.rs")

[Ollama adapter](../../src/embedding/ollama.rs "letopis:type=external;path=src/embedding/ollama.rs")

[OpenAI adapter](../../src/embedding/openai.rs "letopis:type=external;path=src/embedding/openai.rs")

[Background execution](process-execution.md "letopis:type=related_to;path=letopis/architecture/process-execution.md")
