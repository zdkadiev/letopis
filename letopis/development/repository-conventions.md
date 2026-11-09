## Naming conventions {#naming}
<!-- letopis: {"created_at":1793790204,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

`AGENTS.md` is the repository's coding-convention source; its `Naming` section
collects the type, module, file, function and value naming rules. Infrastructure
operations and their application-owned trait methods use entity–action–filter names from the
component's own vocabulary: `file_get_by_path`, `tree_verify_by_oid`, `file_parse`
and `process_spawn`. Filesystem predicates use entity–is–condition names such as
`file_is_exist`. Storage SQL actions use exact table and column names, with one
database action per query function. Application use cases, transaction scopes and
multi-statement operations retain responsibility-based names; closure scopes are
`with_read_tx`, `with_write_tx` and `with_status_read_tx`. Constructors, conversions
and external trait names keep their established forms. New and touched mutex
fields use a `_mutex` suffix; a lock guard is named for the protected state.

The graph neighbor query `edges_get_by_nodes_row_id_and_type` names the joined
`nodes.row_id` filter and the optional edge type. The Git entry lookup uses
`tree_entry_get_by_name`; generic filesystem alias validation uses
`file_verify_distinct_by_paths`. The response writer and Git cleanup owner name their mutex
fields `completion_mutex` and `state_mutex`, respectively.

[Agent conventions](../../AGENTS.md "letopis:type=external;path=AGENTS.md")

[Service configuration types](../../src/service/types.rs "letopis:type=external;path=src/service/types.rs")

[Git tree entry lookup](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

[Filesystem alias validation](../../src/filesystem/local.rs "letopis:type=external;path=src/filesystem/local.rs")

[Git cleanup mutex](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Response writer mutex](../../src/cli/response.rs "letopis:type=external;path=src/cli/response.rs")

[Graph neighbor query](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Refresh outcome](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

## Verification checks {#checks}
<!-- letopis: {"created_at":1793790204,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

The convention forbids helpers whose sole purpose is to return a fixed error or
fixed response. Constructing the value at its call site preserves explicit
context; repeated message text can be kept in a named constant. Refresh
outcomes use `PublicationState` instead of passing an unexplained boolean.

`make check` runs formatting, all-target compilation, selected Clippy rules and
root tests. `make verify` adds strict Rustdoc and release-profile tests. The
selected Clippy rules include checks for excessive boolean parameters or fields
and unnecessary `Result`/`Option` wrappers. A separate syntax checker detects a
conservative subset of fixed-value helpers, including recognized error
constructors and `CommandResponse` literals. It exempts error builders passed as
the function value of `ok_or_else`, including call sites inside trait and generic
implementations. It does not analyze arbitrary macros, all response types or data
flow, so passing it does not prove the absence of every possible fixed-value
helper.

[Lint configuration](../../Cargo.toml "letopis:type=external;path=Cargo.toml")

[Verification commands](../../Makefile "letopis:type=external;path=Makefile")

[Fixed-value syntax checker](../../examples/fixed_value_lint.rs "letopis:type=external;path=examples/fixed_value_lint.rs")

[Development guide](../../docs/development.md "letopis:type=external;path=docs/development.md")

[Git tree validation](../../src/git/tree.rs "letopis:type=external;path=src/git/tree.rs")

[Graph read interface](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

[Component boundaries](../architecture/component-boundaries.md#component-boundaries "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#component-boundaries")
