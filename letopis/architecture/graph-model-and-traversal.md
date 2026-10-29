<!-- letopis: {"created_at":1793278527,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Graph model and traversal

Knowledge edges are identified by source, target and type. `external` targets a source address; other edge types target knowledge. An element with an outgoing `includes` edge is classified as a metavertex. The domain graph holds graph data; the application GraphService traverses a supplied graph reader breadth-first, using a visited set and request depth and element limits.

[Source: src/domain/graph.rs](src/domain/graph.rs "letopis:type=external")

[Source: src/service/graph.rs](src/service/graph.rs "letopis:type=external")
