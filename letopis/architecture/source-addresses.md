<!-- letopis: {"created_at":1793278392,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
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

`source_snapshot` in knowledge metadata records the author's declared Git tree
basis. It is distinct from the tree returned by a particular `inspect` call, which
captures the bytes for that read. A snapshot basis should change only after the
addressed source bytes and knowledge claim have been checked against that tree.

[Source address type](../../src/domain/source.rs "letopis:type=external;path=src/domain/source.rs")

[Source observation](../../src/service/source.rs "letopis:type=external;path=src/service/source.rs")

[Provenance comparisons](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")

[Inspection capture](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")
