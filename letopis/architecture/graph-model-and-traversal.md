<!-- letopis: {"created_at":1793278527,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
# Graph model and traversal

Knowledge elements are identified by repository path or path plus anchor. A graph
edge is identified by the triple `(source, target, type)`; `external` targets a
source address and other types target knowledge. Self-loops and target-kind/type
mismatches are rejected. An element with an outgoing `includes` edge is a
metavertex; this kind is derived rather than authored as Markdown metadata.

The domain `Graph` contains selected identities and optional edges; it does not
load storage. The application `GraphService` receives the shared cache owner and enters a read
scope; nested calls join the caller's pinned view. It expands neighbors breadth-first,
tracks visited node keys, omits start
nodes from discovered elements, and stops at the requested depth and element limit.
The optional edge limit is separate from the element limit.

`TraversalService` refreshes documents, observes embedding configuration and opens
a storage read view before invoking graph traversal. It enriches selected elements
with locations, revision and current observation flags. Search and traversal return
links and metadata, not the Markdown body.

[Graph domain](../../src/domain/graph.rs "letopis:type=external;path=src/domain/graph.rs")

[Graph traversal](../../src/service/graph.rs "letopis:type=external;path=src/service/graph.rs")

[Traversal service](../../src/service/traversal.rs "letopis:type=external;path=src/service/traversal.rs")
