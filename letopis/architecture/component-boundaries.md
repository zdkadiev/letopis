# Program structure and composition

## Component boundaries {#component-boundaries}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
Letopis uses a hexagonal structure. Domain types and pure rules live in
`src/domain`; application-owned interfaces and services live in `src/service`.
Technology-specific implementations live in `git`, `storage`, `filesystem`,
`markdown`, `embedding` and `process`. Services receive their collaborators
through application interfaces instead of orchestrating concrete adapters directly.

The CLI parses input and formats responses. `commands.rs` selects the use case;
`src/lib.rs` is the composition root that wires concrete collaborators into
services. The architecture document describes the intended dependency direction;
the linked code shows the current composition.

[Architecture](../../docs/zkit/letopis/architecture.md "letopis:type=external;path=docs/zkit/letopis/architecture.md")

[Composition root](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Service contracts](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

## Command wiring {#command-wiring}
<!-- letopis: {"created_at":1793278392,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
The CLI validates arguments and decodes JSON input into a typed `Request`.
`run_request` matches that request, constructs the collaborators needed by its
command and returns a typed response for CLI formatting. Help can succeed when
environment lookup fails; commands needing the environment use its result.

`check`, `search`, `traverse`, `inspect` and `reindex` pass their successful
foreground result through the background runner. `status`, `config` and `init`
build their responses directly; `embed --drain` runs the drain path. The command
interface exposes metadata, identities and diagnostics; agents use Git or
Markdown files to obtain content rather than expecting search to return the body.

[CLI request parsing](../../src/cli/input.rs "letopis:type=external;path=src/cli/input.rs")

[Command dispatch and wiring](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Process execution](process-execution.md "letopis:type=related_to;path=letopis/architecture/process-execution.md")
