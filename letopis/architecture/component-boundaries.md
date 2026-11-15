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

The status service interprets observations inside a read scope supplied by
storage. The storage layer owns SQL aggregation and validation of persisted values.

[Architecture](../../docs/zkit/letopis/architecture.md "letopis:type=external;path=docs/zkit/letopis/architecture.md")

[Composition root](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Service contracts](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

[Status service](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

[Command composition](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Agent conventions](../../AGENTS.md "letopis:type=external;path=AGENTS.md")

[Repository conventions](../development/repository-conventions.md#naming "letopis:type=related_to;path=letopis/development/repository-conventions.md#naming")

[Status observation](cli-observation.md#status "letopis:type=related_to;path=letopis/architecture/cli-observation.md#status")

## Command wiring {#command-wiring}
<!-- letopis: {"created_at":1793278392,"source_snapshot":"a29e29b462314a75cf0891beddd5e5f22d041bbf"} -->
The CLI validates arguments and decodes JSON input into a typed `Request`.
`run_request` matches that request, constructs the collaborators needed by its
command and returns a typed response for CLI formatting. Help can succeed when
environment lookup fails; commands needing the environment use its result.

`check`, `search`, `traverse`, `inspect` and `index` pass their successful
foreground result through the background runner. `status`, `config` and `init`
build their responses directly. `embed --drain` runs the drain path. The CLI
exposes metadata, identities and diagnostics. Agents read content through Git
or from the Markdown files.

[CLI request parsing](../../src/cli/input.rs "letopis:type=external;path=src/cli/input.rs")

[Command dispatch and wiring](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Process execution](#process-execution "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#process-execution")

## Foreground and background processes {#process-execution}
<!-- letopis: {"created_at":1793727646,"source_snapshot":"a29e29b462314a75cf0891beddd5e5f22d041bbf"} -->
After a successful `search`, `traverse`, `inspect`, `check` or `index`,
`finish_foreground` asks the background runner whether work is needed. The runner
checks readiness and probes the embedding lock before requesting a child. A launch
problem may be attached as a warning; a readiness error follows the error path.

Composition in `lib.rs::wire_background_runner` selects the executable, the literal
`embed --drain` arguments and repository root as the working directory.
`DetachedProcess` starts its supplied executable/arguments/cwd generically, with
standard streams detached and a separate session. The parent does not wait for
provider inference. The child acquires the embedding lock again when it actually
drains: the earlier probe does not reserve the lock for it.

The executable installs a SIGINT listener for these foreground commands, including
each supported `index` flag combination. The response writer coordinates normal
completion and cancellation so only one response wins.

[Command dispatch](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Foreground completion](../../src/cli/response.rs "letopis:type=external;path=src/cli/response.rs")

[Background runner](../../src/service/background_runner.rs "letopis:type=external;path=src/service/background_runner.rs")

[Detached process](../../src/process/detached.rs "letopis:type=external;path=src/process/detached.rs")

[CLI process entry](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Commands eligible for cancellation](../../src/cli/input.rs "letopis:type=external;path=src/cli/input.rs")

[Command wiring](#command-wiring "letopis:type=related_to;path=letopis/architecture/component-boundaries.md#command-wiring")
