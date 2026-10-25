# TK-20 — Source identity and current availability

Status: approved by the user on 2026-10-25; implementation planning authorized. Scope and upgrade decisions below are approved.
Date: 2026-10-25. Branch: `tk-20`.
Base: `main` / `origin/main` at `a5aa3029521fb97164114047c13b0e4b74ec9232`.

## Intent and sources

Complete the identity and availability of source nodes derived from current external links. Reuse the existing URI codec, graph publication and storage; do not introduce a source registry, HTTP fetcher or new persistent canonical data.

Normative sources:

- [TK-20 task](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-20-source-identity.md).
- [D-91](../../zkit/letopis/decisions.md#D-91), clarified by [D-93](../../zkit/letopis/decisions.md#D-93): source identity and creation time derive from referencing knowledge, not edge IDs or timestamps.
- [MC-4](../../zkit/letopis/mechanics.md#mc-4--источники-и-актуальность), [data E-3](../../zkit/letopis/data.md), [requirements FR-17/20/25/35](../../zkit/letopis/requirements.md).
- [X-4](../../zkit/letopis/contracts/X-4-markdown.md) and [X-1 schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [Program structure](../../zkit/letopis/architecture.md#program-structure) and its [review checklist](../../zkit/letopis/architecture.md#program-structure-review).

The user explicitly requested TK-20 development while pilot/resource validation remains deferred. This does not pass G-1, close TK-17, or establish v1 readiness. TK-19 check is merged; its remaining status integration belongs to TK-28 and is not a prerequisite to implementing source identity.

## Accepted decisions from this conversation

1. Build the common source service now; source support in CLI `inspect` remains TK-24. Do not silently expand TK-20 to mixed batch inspect.
2. HTTP/HTTPS URLs are separate opaque addresses. Preserve the spelling, query and fragment, perform no equivalence/canonicalization or network fetch, and never interpret them as repository paths. Local path and line-range normalization applies only to repository files.
3. Upgrading from the old source parser requires an explicit rebuild of the derived cache. Do not migrate previously corrupted URL identities automatically. Preserve Markdown, Git and `vectors.db`.
4. Existing project decisions still apply: agents own Markdown files and directory layout; no IDs in metadata; ordinary relation names are agent-defined, with system `external` and `includes` semantics unchanged.

## Current implementation and gaps

Verified at the base revision:

- `domain/identity.rs` already normalizes dot components and decimal line ranges for local `SourceUri` values.
- `service/projection.rs` reconciles external references, derives minimum referencing knowledge `created_at`, removes orphan sources atomically, and hashes addressed local bytes for touched sources.
- `storage` persists source metadata without source text or UUIDs. `StorageRead::read_node` can resolve an address from a pinned read view.
- Projection reconciliation only rereads touched sources. A source file can change without editing a Markdown referrer; persisted hashes alone cannot establish live availability.
- `SourceUri::parse` currently routes HTTP-like strings through `RepoPath`, collapsing `//`; this must stop.
- Search has only `stale`; inspect handles knowledge and rejects referenced source requests as currently unavailable. TK-20 does not claim those later consumers are implemented.

## Required behavior

### Local identity

The sole ID is the canonical root-relative file path, optionally followed by `#Lstart-Lend`. Normalize `.`/`..`, redundant slash components and decimal line numbers using the existing TK-7 rules. Reject root escape, absolute paths, control characters and malformed, zero, reversed or overflowing ranges. Preserve path case and Unicode; do not resolve symlinks or decode percent escapes as part of lexical normalization.

Reject local spellings whose normalized root path begins with case-insensitive
`http:` or `https:` (for example `./https:notes`, `dir/../HTTP:notes`,
`./https://Host/a`, including local line ranges). These reserved prefixes would
change classification when the sole persisted identity is parsed again. Do not
promote a normalized local spelling into a remote URL. Every accepted serialized
address must parse again with the same identity and local/remote classification.
Root-local filenames with these prefixes require renaming or an unambiguous
containing directory such as `docs/https:notes`; RepoPath/KnowledgeId grammar is
unchanged. This final-review clarification was approved by the controller on
2026-10-25; raw absolute remote URLs still retain their exact original spelling.

A whole-file address and each explicit range remain distinct source nodes, even when their addressed bytes happen to match. Multiple external links with equivalent local spellings identify one source. No UUID, source registration file, hidden creation timestamp or metadata ID is added.

### Remote addresses

Recognize absolute HTTP/HTTPS addresses separately from local file paths before local normalization. Preserve their original text, including query, fragment, percent escapes and case; different spellings remain different identities. A URL fragment is opaque, including one that resembles `#L1-L2`.

The builder may choose the smallest explicit syntactic check needed to distinguish an HTTP/HTTPS authority from a malformed address, but must reject empty authority and controls rather than reinterpret them as a local file. No DNS, connection, HTTP, provider call or filesystem read is allowed for a remote address. No new URL canonicalization dependency is required by this design.

Remote nodes still derive existence and `created_at` from external links. Their addressed bytes and hash are unknown, not an asserted 404. They have no local line range. Return an explicit unfetched availability state; do not invent a remote hash, availability assertion or freshness flag.

### Derived graph lifecycle

A source node exists exactly while at least one effective external edge references it. Its `created_at` is the minimum authored `created_at` of all currently effective referencing knowledge elements. Duplicate edges do not alter it. Editing or removing the earliest referrer recomputes the minimum. Removing the last reference removes the source in the same cache publication.

Existing invalid-file policy remains: retain the last valid projection and its effective links until that file is repaired or removed. A missing source file does not remove referencing knowledge or the source node. Preserve the existing atomic rejection of a source identity that conflicts with a knowledge identity; do not invent a new graph namespace in this task.

Deleting only the derived cache and running a fresh full refresh must reconstruct the same source identities and creation times from the same corpus. This test uses the existing application refresh path; it does not add the future CLI reindex command from TK-25.

### Upgrade from existing caches

The old parser can persist a remote URL such as `https://host/path` as `https:/host/path`. Ordinary refresh skips reparsing unchanged valid files, so changing the parser alone cannot repair those identities. The user chose an explicit derived-cache rebuild for the transition to TK-20, rather than automatic repair or a source migration.

Document this one-time upgrade prerequisite in the developer/user guidance and the task report. Before rebuilding, stop all Letopis commands and detached drain processes using that repository. Recreate only the derived `cache.db` and its SQLite sidecars, if present, while no connections are open; the ordinary refresh path then rebuilds source nodes from canonical Markdown. Do not remove Git objects/refs, Markdown, `vectors.db`, configuration or lock files. This specification does not authorize deleting a user's live cache during development.

Do not add a migration table, heuristic URL repair, automatic database deletion, or the future CLI reindex command. Until this explicit rebuild has been performed, an old cache is outside TK-20's upgraded-state guarantee; document that limit rather than claiming automatic compatibility. Fresh caches and rebuilt caches must satisfy the same identity and availability contracts.

### Current source service

Introduce a small application-owned source-reading interface and a concrete service only where needed. The operation accepts a validated source address and an existing pinned `StorageRead` view supplied by its caller. The caller owns corpus refresh and read lifetime. The service does not refresh, reopen storage or publish inside this read.

1. Resolve current graph membership and derived creation time from the supplied view.
2. If no current source node exists at this address, return an explicit not-found outcome. Do not read a file or perform network I/O merely because an arbitrary address was requested. Future CLI consumers map this outcome to their own batch contract.
3. For a referenced local source, capture its current file through the confined `FileSystem` interface, select the exact requested bytes, compute their SHA-256 and return the normalized identity, derived creation time, addressed lines, hash and typed availability.
4. A missing local file or incomplete range returns `hash = None` with a missing availability state and `source_not_found` freshness reason. It is not an infrastructure error and does not remove the node.
5. For a remote source return `hash = None`, no local lines and an unfetched state without I/O. Do not use `source_not_found` to assert an untested remote failure.
6. Reject unsafe file reads or propagate I/O/storage failures as typed safe errors with private causes; never turn them into missing or return a partial success.

Available local bytes return no source-not-found reason. This is not proof of knowledge truth or historical freshness. `source_changed`, historical snapshot comparison and unavailable-snapshot reasons remain TK-21. No new public CLI flags are added in TK-20: the typed service outcome is the handoff for later search/traverse/inspect consumers.

Do not trust persisted source hashes for current availability: changing, deleting, truncating or restoring source bytes without changing Markdown must be visible on the next explicit source-service read. Read one file per observation and release its bytes; no corpus-wide source-content cache or unbounded retained source bodies.

### Exact byte addressing

Whole-file hashing covers every byte, including an empty file. Line ranges are one-based and inclusive. Preserve CRLF/LF and arbitrary source bytes, without UTF-8 decoding or EOL normalization. An unterminated final line counts; a trailing newline does not create an extra empty line. An empty file has no addressable line 1. An incomplete range is missing in full, never truncated to existing lines.

Projection and current reads must share the same pure address/byte selection rules so they cannot disagree. The builder chooses a focused helper/type in the owning domain or service module, with minimal visibility and reuse by both consumers; avoid duplicate parsing of `SourceUri` strings in callers.

## Boundaries and physical architecture

- `domain/`: pure address classification, normalization and byte/range rules; no file, SQL or HTTP access.
- `service/`: application-owned source interface/result types and concrete source service in its responsibility-named file. Existing document projection reuses the rules and distinguishes remote addresses before filesystem access.
- `filesystem/`: existing confined synchronous file capture. No new remote adapter.
- `storage/`: existing source/edge metadata and atomic publication. Keep SeaQuery for ordinary queries; do not add a registry table or source contents. No schema change is currently justified.
- `lib.rs`: actual constructor wiring for the source service through interfaces. CLI handlers continue invoking service interfaces only.
- `cli/inspection` behavior, mixed batch capture and common source Git trees remain TK-24. Search filters/consumer flags remain TK-21/22, traversal TK-23; status TK-28, handled SIGINT TK-29.

Follow AGENTS.md: synchronous I/O, idiomatic ownership, typed errors, documented public Debug types, explicit imports, private details, verbs, focused 40–60-line functions, caller-before-helper reading order and useful named constants. No Tokio, forwarding-only abstraction, dynamic-dispatch layer, web fetch, new process, storage schema or index added without a demonstrated requirement.

## Acceptance and verification

1. Independent normalization literals prove canonical whole-file/range identity and remote URL preservation. Reject unsafe local and malformed remote addresses without I/O.
2. Real Markdown → refresh → SQLite tests prove one shared source, minimum creation time, earliest-referrer edit/removal, invalid referrer retention, last-reference removal and reappearance.
3. Remove only the fixture cache and refresh again; compare literal expected source IDs and creation times. Different internal row IDs/revisions are not canonical identities. Include the explicit old-parser upgrade scenario: a malformed cached URL becomes the original opaque URL after rebuilding from unchanged Markdown, with canonical files and `vectors.db` preserved.
4. Source-service tests on a pinned view prove unknown/orphan not-found, zero file reads for unknown/remote sources, and exact current hash/lines despite unchanged referrer Markdown.
5. Local byte cases include whole/empty files, CRLF/LF, binary bytes, unterminated last line, missing file, out-of-range and later restore. Use independent known hash literals or an independent oracle, not production helpers as ground truth.
6. Remote-link projection and reading never call filesystem/HTTP; use instrumented boundaries rather than an external network dependency. A syntactically valid URL remains a graph address with unknown bytes.
7. Verify safe failure propagation and no partial publication, unchanged knowledge/FTS behavior, metadata-only storage and actual architectural import/wiring boundaries.
8. Run `make check` and `make verify` for the final changed tree, selected focused tests and manual mutation controls with exact restoration. Record commands, outcomes, captured revision/input inventory, host limits and any ignored/unrun checks. Use bounded logs, not a full automatic mutation campaign.
9. If storage/native/build inputs actually change, run the relevant storage host/native runner in a new evidence directory; do not claim Linux or native evidence from root tests alone.

Reports go under `docs/zkit/letopis/checks/TK-20/`; the implementation plan goes under `docs/superpowers/plans/` after full specification approval. Update the task card and GitHub results with explicit remaining consumer integration. Do not mark all FR-20/FR-25 or G-1/G-2 complete from this service-level task.

## Builder choices and non-goals

The builder chooses exact trait/method/type names, whether to split the existing identity file, and the smallest shared pure byte-addressing helper. Reuse current storage and filesystem interfaces; concrete source service provides current metadata, not trivial forwarding. No remote URL equivalence rules, real HTTP fetching, historical provenance, release claims, paid model runs, CLI source inspect or repo-wide refactor.

## Independent builder check

A fresh reviewer (`/root/tk20_builder_check`) found one material question: whether old cached URL identities require automatic repair or an explicit rebuild. The user answered on 2026-10-25: explicit cache rebuild on upgrade. The upgrade section records that answer. The reviewer found no other material planning questions; its service/CLI boundary and unfetched remote-state checks were clear.

Private review receipt: `.artifacts/tk20/start/builder-check.md`. The user approved the full document on 2026-10-25 and requested the implementation plan. The user subsequently approved the plan and explicitly selected subagent-driven implementation on 2026-10-25.

## Delivery status — 2026-10-25

Implementation is accepted at `387a68a0f33bea3ab446e2cc19bfe53ed33779a4` after
three independent task reviews, one whole-branch review and scoped approval of its
single Important P2 fix. The normalized local-prefix rejection above preserves
canonical parse/serialize identity and is covered by actual CLI/cache regressions.
The [current report](../../zkit/letopis/checks/TK-20/report.md) and
[final verification](../../zkit/letopis/checks/TK-20/final-fix-verification.json)
record 551 passed/5 ignored per debug/release profile and 291 unchanged inputs.
Engram handoff is complete (revision 271). CLI consumers and G-1/G-2 retain their
stated limits; main integration and publication are separate finishing actions.
