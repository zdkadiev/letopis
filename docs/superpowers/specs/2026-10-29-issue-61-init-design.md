# Issue 61 — Explicit repository initialization

**Status:** approved by the user on 2026-10-29 ("go with plan"). This revision supersedes the earlier unapproved design. Implementation remains paused until the revised implementation plan is reviewed and approved.

## 1. Goal and sources

[Issue #61](https://github.com/zaurkadiev/letopis/issues/61) requests `letopis init` to prepare the current Git repository with service database schemas and an editable, valid `.letopis/config.json`. The repository must be usable through FTS before an embedding provider is selected.

Design sources: [program structure](../../zkit/letopis/architecture.md#program-structure), [D-70](../../zkit/letopis/decisions.md#D-70), [D-95](../../zkit/letopis/decisions.md#D-95), [X-1](../../zkit/letopis/contracts/X-1-cli.md), and [mechanics](../../zkit/letopis/mechanics.md). Amend the relevant decisions explicitly; preserve their historical meaning. Work already produced against the previous unapproved spec does not establish accepted behavior.

## 2. Accepted configuration format

The generated configuration is:

```json
{
  "embedding_model": {
    "enabled": false,
    "provider": null,
    "model": null,
    "parameters": {},
    "max_retries": 0
  }
}
```

The user chose a root `embedding_model` section so other configuration sections can be added when required. This issue introduces only that section. The `enabled` flag permits disabling execution without clearing configured provider settings. `max_retries: 0` means one attempt once execution is enabled.

Only the new nested format is supported. Old flat files fail validation with a clear migration diagnostic; they are never converted or overwritten automatically. Existing configured users must move their settings under `embedding_model` and set `enabled` explicitly.

### Valid states

| State | Required values | Result |
| --- | --- | --- |
| Unconfigured | `enabled: false`, both `provider` and `model` null, `parameters: {}` | Valid; embeddings disabled |
| Configured but disabled | `enabled: false`, supported provider, nonempty model, valid provider parameters | Valid; settings retained without execution |
| Configured and enabled | `enabled: true`, supported provider, nonempty model, valid provider parameters | Valid; normal embedding execution |
| Partial configuration | Exactly one of provider/model null | Invalid, including when disabled |
| Enabled without configuration | `enabled: true` with null provider/model | Invalid |
| Unconfigured with provider parameters | Both provider/model null but parameters nonempty | Invalid |

Retained provider settings are validated even when disabled. Validation is local and never contacts a provider. The flag controls execution, not validity. An invalid config fails in `config`, appears as a current problem in `status`, and leaves search available through FTS.

### Validation details

All five fields in `embedding_model` and the root section itself are required. Reject unknown fields at the root and in `embedding_model`; future sections require explicit schema additions. Keep existing provider parameter extensibility, supported provider values, duplicate-key rejection, retry limits, value-type checks, secret-field restrictions and safe error causes. Validate JSON shape and applicable provider settings on every read before use. The X-1 JSON Schema and runtime describe the same structural states; document runtime-only semantic checks such as endpoint policy and canonical-number constraints.

Generate readable, indented UTF-8 JSON with a final newline. Whitespace does not change its meaning.

## 3. Accepted disabled behavior

- Disabled embeddings never participate in search; search uses FTS.
- Disabling preserves existing vectors for reuse when the same model configuration is re-enabled.
- `status` reports `embedding_backlog: 0` and `embedding_failed: 0` while disabled.
- While disabled, `status` hides stored embedding and query failures. Invalid configuration is still reported.

### Command and identity details

| Command / condition | Behavior |
| --- | --- |
| `config`, file absent | Existing success with `result: null`; creates no state |
| `config`, valid file | Returns the complete nested configuration object, including `enabled` |
| `config`, malformed, partial or provider-invalid file | Safe validation failure, using the existing `embedding_failed` code |
| `config`, unreadable or unsafe file | Safe read failure, using the existing `internal_error` code |
| `status`, valid disabled file | No invalid-config problem; accepted zero embedding counts and hidden execution failures |
| `status`, invalid/unreadable file | Current `invalid_embedding_config` problem; preserve existing unavailable-policy behavior |
| Search, disabled or invalid config | FTS remains available; no embedding inference or background drain launch |
| `embed --drain`, valid disabled config | Successful no-op without provider calls or vector changes |

`config` and `status` remain observation-only: no config writes, database bootstrap, refresh, vector reset, inference or background launch. An absent configuration retains existing behavior unless explicitly changed above.

The execution switch is not part of vector compatibility. Turning it off and back on with unchanged provider/model/parameters/retry settings reuses compatible stored vectors and retry state. Disabling does not clear stored failures; it hides execution failures from status. A changed active model configuration retains the existing guarded reset/readiness rules. Edits made while disabled do not initiate resets or work; compatibility is evaluated when enabled again. Unrelated future root sections must not affect embedding identity.

Public errors may identify the invalid section or required migration, but must not expose raw JSON, secrets, endpoints or subprocess payloads.

## 4. Initialization requirements and accepted recovery behavior

Required by the issue:

1. Resolve the current directory through the existing Git boundary and require an initialized, non-bare Git work tree before creating any service state. Invocation from a subdirectory targets the Git root. Outside a work tree, return a safe error stating that a Git repository is required and create nothing.
2. Prepare `.letopis/`, `cache.db`, `vectors.db` and the generated config through the existing filesystem/storage boundaries.
3. Repeating initialization succeeds without replacing edited configuration or existing valid databases. Malformed, unreadable, unsafe or incompatible state is reported rather than overwritten.
4. Concurrent invocations leave complete valid config and usable schemas without overwriting another invocation's config.

**Accepted recovery behavior:** if initialization fails midway, keep safely completed state so a retry can finish. Preserve existing files and publish config only as a complete file. Initialization is not an all-or-nothing transaction across the directory, databases and config.

### CLI and safety details

- `letopis init` accepts the ordinary empty JSON request `{}`. Existing terminal/empty-stdin handling applies. Reject extra argv and unknown request fields through the usual parser.
- Success is `{"ok":true,"result":{}}`, exit 0. Errors use the existing single JSON envelope and exit 1. Outside a Git work tree uses `invalid_request`; unrelated Git failures retain an appropriate safe infrastructure error.
- `init --help` and general help describe the command and work outside a repository without reading stdin or creating state.
- After the Git guard, validate an existing config before creating additional database state. Open/bootstrap schemas through the existing storage implementation and install generated config only if absent. On a publication race, read and validate the winning file before success.
- Config publication is atomic and does not replace an existing entry. Readers see a complete file or absence. Follow the existing path confinement and private permissions policy; reject unsafe service/config paths.
- Before reporting success, persist completed config publication and newly created service-directory entries using the platform's existing durability conventions. A retry after a sync failure must complete the missing durability work before success.
- Reuse existing SQLite bootstrap serialization and schema validation. Do not repair or reset malformed/incompatible databases as part of `init`.
- Initialization does not index documents, change source/Git state, contact a provider, reset vectors, start drain, or initialize Git itself. Existing lazy setup of other commands remains supported.

Select concrete helper types and filesystem operations in the implementation plan. Keep use-case orchestration in services, concrete I/O in its owners, constructor wiring in `lib.rs`, and command selection in `commands.rs`. Do not introduce speculative configuration sections, dependencies, runtimes, processes or storage schemas.

## 5. Acceptance and verification

| Area | Required evidence |
| --- | --- |
| Configuration | Generated, configured enabled/disabled, absent, partial, old flat, unknown/duplicate fields, malformed and unreadable files; same validation behavior across readers |
| Schema agreement | Independent accept/reject fixtures for structural states; runtime checks for documented semantic limits; generated output validates against X-1 |
| Disabled execution | No inference, vector use/reset or drain launch; vectors survive disable/re-enable; zero status counts and hidden execution failures; invalid config still visible |
| Initialization | Real binary at root and subdirectory; empty initialized repository; outside-Git and bare-repository rejection without service-state creation |
| Preservation | Repeated init preserves edited config and database logical contents; invalid/incompatible/unsafe existing state is reported and preserved |
| Concurrency and recovery | Concurrent init, invalid publication winner, complete-file visibility, interrupted/failed setup and retry after sync failure |
| Delivery | CLI help, X-1 prose/schema/examples, Letopis skill and decision revisions agree with behavior |

New behavior tests use independent expected literals/oracles, Given/When/Then markers and mutation checks under AGENTS.md. Run `make check`, `make verify` and the host storage runner with a new evidence directory. Preserve historical evidence. Report exact results, platform limits and unrun checks. Review physical modules, visibility, trait implementations, import direction and actual constructor wiring.

## 6. Approval and next step

Revise the implementation plan to match this approved specification and present it for separate user approval. Only then resume implementation through GPT Terra at extra-high reasoning, as requested.
