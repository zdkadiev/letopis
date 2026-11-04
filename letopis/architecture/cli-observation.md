# CLI observation commands

## Check scope {#check-scope}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
`check` refreshes the corpus and returns validation problems from a pinned storage
view. With omitted or empty `paths`, it requests a verified refresh with a full
source-snapshot audit. A scoped check uses ordinary refresh and restricts reported
findings to the selected paths; it does not perform that full audit.

After unscoped verified refresh, `CheckService` checks that the read revision is
still the audited revision. If a publication intervened, it returns a retryable
internal error instead of reporting the newer, unaudited revision as checked.

[Check service](../../src/service/check.rs "letopis:type=external;path=src/service/check.rs")

[CLI check contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")

## Inspect results {#inspect-results}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
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
