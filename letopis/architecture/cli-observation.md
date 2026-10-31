# CLI observation commands

## Check scope {#check-scope}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
`check` reports corpus validation errors and refreshes the index cache. With omitted or empty `paths` it also audits complete source snapshots; nonempty `paths` skip that full audit.

[CLI check contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")

## Inspect results {#inspect-results}
<!-- letopis: {"created_at":1793278642,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
`inspect` accepts a nonempty list of knowledge or source addresses and returns a Git tree snapshot and metadata for each address, without file text. An unknown ID rejects the whole request.

[CLI inspect contract](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")
