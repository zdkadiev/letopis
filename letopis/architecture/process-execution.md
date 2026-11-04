<!-- letopis: {"created_at":1793727646,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
# Foreground and background processes

The CLI maps arguments and JSON input into a typed request. `run_request` locates
its environment and wires the application services for the selected command;
help can be returned even when environment discovery fails. Service responses
are formatted by the CLI, with help/version using plain text.

After a successful `search`, `traverse`, `inspect`, `check` or `reindex`,
`finish_foreground` asks the background runner whether work is needed. The runner
checks readiness and probes the embedding lock before requesting a child. A launch
problem may be attached as a warning; a readiness error follows the error path.

Composition in `lib.rs::wire_background_runner` selects the executable, the literal
`embed --drain` arguments and repository root as the working directory.
`DetachedProcess` starts its supplied executable/arguments/cwd generically, with
standard streams detached and a separate session. The parent does not wait for
provider inference. The child acquires the embedding lock again when it actually
drains: the earlier probe does not reserve the lock for it.

For the five foreground command forms, the executable installs a SIGINT listener.
The response writer coordinates normal completion and cancellation so only one
response wins. The detached drain is an independent process, not a child operation
whose completion is awaited by the foreground caller.

[Command dispatch](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Foreground completion](../../src/cli/response.rs "letopis:type=external;path=src/cli/response.rs")

[Background runner](../../src/service/background_runner.rs "letopis:type=external;path=src/service/background_runner.rs")

[Detached process](../../src/process/detached.rs "letopis:type=external;path=src/process/detached.rs")

[CLI process entry](../../src/lib.rs "letopis:type=external;path=src/lib.rs")
