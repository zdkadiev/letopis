<!-- letopis: {"created_at":1793278392,"source_snapshot":"a29e29b462314a75cf0891beddd5e5f22d041bbf"} -->
# Source addresses

`SourceUri` distinguishes normalized repository-relative paths, optionally with
inclusive line ranges, from opaque absolute HTTP/HTTPS addresses. Parsing does not
fetch remote URLs. Local range selection uses exact bytes, including line endings,
and fails when the complete range is unavailable.

`SourceService` resolves a declared source through the storage read view. For a
local address it observes the file through the filesystem boundary and computes a
hash for the whole file or selected lines. A missing file or unavailable range is
reported as source-not-found. Remote addresses are reported as unfetched with no
local source hash; their existence or content is not verified over the network.

`ProvenanceService` compares the exact addressed local bytes against the declared
basis. It can also compare those bytes between the basis and a requested Git tree;
a missing basis or source is distinguished from equal content. Remote sources
remain unfetched, so a remote address alone does not prove current content.

For accepted diagnostic batches, provenance groups historical reads by basis tree
and file path. Distinct line addresses can share one verified blob read and one
line-boundary scan. Live source observations continue to capture the current bytes.

`source_snapshot` in knowledge metadata records the author's declared Git tree
basis. It is distinct from the tree returned by a particular `inspect` call, which
captures the bytes for that read. Keep the recorded basis while its evidence
supports the written claim. When the claim or binding requires a different
source version, verify the replacement tree before recording it.

[Source address type](../../src/domain/source.rs "letopis:type=external;path=src/domain/source.rs")

[Source observation](../../src/service/source.rs "letopis:type=external;path=src/service/source.rs")

[Provenance comparisons](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")

[Inspection capture](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")

[Measured historical range reuse](../development/performance-bottlenecks.md#historical-range-reuse-2026-11-10 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#historical-range-reuse-2026-11-10")
