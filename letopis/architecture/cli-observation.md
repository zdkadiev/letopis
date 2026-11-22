# Repository setup and observation

## Initialization and configuration {#initialization}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->

`init` discovers the repository and validates an existing configuration before
opening service storage. It initializes storage, installs generated configuration
and a commented default `.letopis/ignore` only when absent, reads and validates
the resulting configuration, then syncs service state. Existing configuration
and ignore bytes are not silently overwritten.

`config` observes the current configuration through non-creating filesystem access.
It parses the document and validates provider settings when an embedding model is
configured; it does not run background embedding work. The full configuration
also contains discovery policy with effective defaults. Discovery does not enter
the embedding identity, and invalid embedding settings do not prevent discovery
when the JSON root and discovery section remain valid.

[Initialization](../../src/service/init.rs "letopis:type=external;path=src/service/init.rs")

[Configuration values](../../src/service/types.rs "letopis:type=external;path=src/service/types.rs")

[Configuration reader](../../src/service/configuration.rs "letopis:type=external;path=src/service/configuration.rs")

[Command dispatch](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

## Status observation {#status}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->

`status` observes the last published revision, diagnostics, embedding backlog and
failures, configuration validity and lock state. It reads a status view if one is
available; absent published state yields no revision and zero backlog. Status does
not itself refresh the corpus. A disabled embedding configuration uses a disabled
status policy; invalid configuration remains visible as a problem.

`StatusService` owns Disabled/Unavailable/Ready policy, identity interpretation and
backlog/failed arithmetic. Disabled still enters the validated status scope but
skips work and query-error observations. Unavailable or mismatched identity keeps
all current hashes in backlog and still exposes query failures. Storage owns SQL
aggregation, decoding and integrity checks; uncovered attempt records are validated
before threshold filtering, even when they would not be exhausted.

Embedding configuration and durable vector identity are separate: embedding
execution observes the config file and reconciles vector state through storage.
The status/config read paths report observations and do not perform this reset.

[Status service](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

[Status SQL observations](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Attempt validation](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

[Embedding execution](embedding-execution.md#service "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#service")

## Check scope {#check-scope}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->
`check` refreshes the corpus and returns validation problems from a pinned storage
view. With omitted or empty `paths`, it requests a verified refresh with a full
source-snapshot audit. A scoped check uses ordinary refresh and restricts reported
findings to the selected paths; it does not perform that full audit.

After unscoped verified refresh, `CheckService` checks that the read revision is
still the audited revision. If a publication intervened, it returns a retryable
internal error instead of reporting the newer, unaudited revision as checked.

[Check service](../../src/service/check.rs "letopis:type=external;path=src/service/check.rs")

[CLI check contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")

## Check findings {#check-findings}
<!-- letopis: {"created_at":1793816837,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->
`check` joins findings from two observers over the same pinned read view. Graph
diagnostics report a file whose latest parse failed (`invalid_markup`, or a
classified parser diagnostic), a knowledge link whose target is absent
(`dangling_target`), and members of an `includes` cycle (`includes_cycle`, one
finding per file).

Source provenance reports an element with source bindings but no `source_snapshot`
(`missing_source_snapshot`), a snapshot tree that is unavailable
(`source_snapshot_unavailable`), and a missing local file or line range in the
worktree or available basis (`source_not_found`). Observation is batched over
the selected elements. Changed bytes at an available address produce a
`source_changed` flag in search and traversal.

Each finding carries `validation_failed` as its code, static safe text, and
details with `path` and `reason`, plus `element_id` and `target` when applicable.
Invalid-file findings carry only `path` and `reason`.
Findings are filtered to the selected paths, sorted and deduplicated. The
command can return `ok:true` with a nonempty `errors` array.

[Graph diagnostics](../../src/service/graph_diagnostics.rs "letopis:type=external;path=src/service/graph_diagnostics.rs")

[Source observation helper](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

[Provenance issues and freshness flags](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")

## Inspect results {#inspect-results}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->
`inspect` accepts a nonempty set of knowledge or declared-source addresses. It
refreshes the projection, captures marked corpus files and requested source data,
assembles a Git tree from captured blobs and pins it before returning. An unknown
address rejects the whole request.

The response contains the retained tree OID and per-address path, optional
inclusive line range and SHA-256 of addressed bytes when available. Read the
knowledge and local source bytes from that same tree. A declared local source
with a missing file or unavailable range can return a null hash. HTTP and HTTPS
sources are represented by their addresses and a null hash.

[Inspection service](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")

[CLI inspect contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")
