# Program structure and composition

## Component boundaries {#component-boundaries}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->
Letopis uses a hexagonal structure. Domain types and pure rules live in
`src/domain`; application-owned interfaces and services live in `src/service`.
Technology-specific implementations live in `git`, `storage`, `filesystem`,
`markdown`, `embedding` and `process`. Services receive their collaborators
through application interfaces instead of orchestrating concrete adapters directly.

The CLI parses input and formats responses. `commands.rs` selects the use case;
`src/lib.rs` is the composition root that wires concrete collaborators into
services. The status and configuration paths use composition-root callbacks to
keep borrowed collaborators alive for the operation. Repository opening also
happens through a composition-root helper, which receives the shared Git cleanup
owner where required.

The status service accesses its injected storage through a receiver method while
inside the status read scope. This keeps storage access with the service that
owns the orchestration. It also owns configuration-mode policy and backlog/failed
interpretation; storage retains SQL aggregation and persisted-value validation.

[Architecture](../../docs/zkit/letopis/architecture.md "letopis:type=external;path=docs/zkit/letopis/architecture.md")

[Composition root](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Service contracts](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

[Status service](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

[Command composition](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Agent conventions](../../AGENTS.md "letopis:type=external;path=AGENTS.md")

[Repository conventions](../development/repository-conventions.md "letopis:type=related_to;path=letopis/development/repository-conventions.md")

## Command wiring {#command-wiring}
<!-- letopis: {"created_at":1793278392,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
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
