# CLI observation commands

## Check scope {#check-scope}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
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
<!-- letopis: {"created_at":1793816837,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
`check` joins findings from two observers over the same pinned read view. Graph
diagnostics report a file whose latest parse failed (`invalid_markup`, or a
classified parser diagnostic), a knowledge link whose target is absent
(`dangling_target`), and members of an `includes` cycle (`includes_cycle`, one
finding per file).

Source provenance reports a knowledge element without a `source_snapshot`
(`missing_source_snapshot`), a snapshot tree that is unavailable
(`source_snapshot_unavailable`), and an address absent from its snapshot tree
(`source_not_found`). Observation is batched over the selected elements.

Each finding carries `validation_failed` as its code, static safe text, and
details with `path` and `reason`, plus `element_id` and `target` when applicable.
Invalid-file findings carry only `path` and `reason`.
Findings are filtered to the selected paths, sorted and deduplicated. They never
fail the command: `ok` stays true with a nonempty `errors` array. Only refresh,
storage or provenance failures become an internal error response.

[Graph diagnostics](../../src/service/graph_diagnostics.rs "letopis:type=external;path=src/service/graph_diagnostics.rs")

[Source observation helper](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

## Inspect results {#inspect-results}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->
`inspect` accepts a nonempty set of knowledge or declared-source addresses. It
refreshes the projection, captures marked corpus files and requested source data,
assembles a Git tree from captured blobs and pins it before returning. An unknown
address rejects the whole request.

The response contains the retained tree OID and per-address path, optional
inclusive line range and SHA-256 of addressed bytes when available; it does not
return file text. Read the knowledge and local source bytes from that same returned
tree. A missing or unfetchable source may have a null hash; remote URLs are not
fetched by this operation.

[Inspection service](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")

[CLI inspect contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")
