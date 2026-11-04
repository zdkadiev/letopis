<!-- letopis: {"created_at":1793790204,"source_snapshot":"1c24df703adfbe1c196583a36ddcb19cea7bfeb5"} -->
# Repository conventions and checks

`AGENTS.md` is the repository's coding-convention source. Storage query
functions and their application-owned trait methods use table-first names with
schema column names in their filters; each query function has one database
action. Transaction scopes and multi-statement operations retain names that
describe their responsibilities. New and touched mutex fields use a `_mutex`
suffix, while a lock guard is named for the protected state.

The graph neighbor query `edges_get_by_nodes_row_id_and_type` names the joined
`nodes.row_id` filter and the optional edge type. The Git entry lookup uses
`get_entry_by_name`; the filesystem comparison uses the predicate
`is_same_entry`. The response writer and Git cleanup owner name their mutex
fields `completion_mutex` and `state_mutex`, respectively.

The convention forbids helpers whose sole purpose is to return a fixed error or
fixed response. Constructing the value at its call site preserves explicit
context; repeated message text can be kept in a named constant. Refresh
outcomes use `PublicationState` instead of passing an unexplained boolean.

`make check` runs formatting, all-target compilation, selected Clippy rules and
root tests. `make verify` adds strict Rustdoc and release-profile tests. The
selected Clippy rules include checks for excessive boolean parameters or fields
and unnecessary `Result`/`Option` wrappers. A separate syntax checker detects a
conservative subset of fixed-value helpers, including recognized error
constructors and `CommandResponse` literals. It does not analyze arbitrary
macros, generic functions or generic implementations (including lifetime-only
generics), all response types or data flow, so passing it does not prove the
absence of every possible fixed-value helper.

[Agent conventions](../../AGENTS.md "letopis:type=external;path=AGENTS.md")

[Lint configuration](../../Cargo.toml "letopis:type=external;path=Cargo.toml")

[Verification commands](../../Makefile "letopis:type=external;path=Makefile")

[Fixed-value syntax checker](../../examples/fixed_value_lint.rs "letopis:type=external;path=examples/fixed_value_lint.rs")

[Development guide](../../docs/development.md "letopis:type=external;path=docs/development.md")

[Git tree validation](../../src/git/tree.rs "letopis:type=external;path=src/git/tree.rs")

[Git tree entry lookup](../../src/git/objects.rs "letopis:type=external;path=src/git/objects.rs")

[Filesystem entry comparison](../../src/filesystem/local.rs "letopis:type=external;path=src/filesystem/local.rs")

[Git cleanup mutex](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Response writer mutex](../../src/cli/response.rs "letopis:type=external;path=src/cli/response.rs")

[Service configuration types](../../src/service/types.rs "letopis:type=external;path=src/service/types.rs")

[Graph neighbor query](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Graph read interface](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

[Refresh outcome](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Component boundaries](../architecture/component-boundaries.md#component-boundaries "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#component-boundaries")
