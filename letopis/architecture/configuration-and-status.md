<!-- letopis: {"created_at":1793727646,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
# Initialization, configuration and status

`init` discovers the repository and validates an existing configuration before
opening service storage. It initializes storage, installs generated configuration
only if absent, reads and validates the resulting configuration, then syncs service
state. Existing configuration is not silently overwritten.

`config` observes the current configuration through non-creating filesystem access.
It parses the document and validates provider settings when an embedding model is
configured; it does not run background embedding work.

`status` observes the last published revision, diagnostics, embedding backlog and
failures, configuration validity and lock state. It reads a status view if one is
available; absent published state yields no revision and zero backlog. Status does
not itself refresh the corpus. A disabled embedding configuration uses a disabled
status policy; invalid configuration remains visible as a problem.

Embedding configuration and durable vector identity are separate: embedding
execution observes the config file and reconciles vector state through storage.
The status/config read paths report observations and do not perform this reset.

[Initialization](../../src/service/init.rs "letopis:type=external;path=src/service/init.rs")

[Configuration reader](../../src/service/configuration.rs "letopis:type=external;path=src/service/configuration.rs")

[Status service](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

[Command dispatch](../../src/commands.rs "letopis:type=external;path=src/commands.rs")

[Embedding execution](embedding-execution.md "letopis:type=related_to;path=letopis/architecture/embedding-execution.md")
