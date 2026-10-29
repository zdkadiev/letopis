<!-- letopis: {"created_at":1793278527,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Embedding execution

EmbeddingService coordinates provider configuration, stored vector work, query embeddings and drain operations through application interfaces. If a ready configuration is unavailable, query embedding is unavailable and the executor reports no pending work. Search still obtains full-text candidates when vector candidates are unavailable.

[Source: src/service/embedding.rs](src/service/embedding.rs "letopis:type=external")

[Source: src/service/search.rs](src/service/search.rs "letopis:type=external")
