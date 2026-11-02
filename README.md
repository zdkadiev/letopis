<h1 align="center">Letopis</h1>

<p align="center"><strong>Shared project memory for coding agents, built on a Markdown knowledge graph.</strong></p>

<p align="center">
  Full-text and optional semantic search across the graph, files anywhere in your repo, and knowledge grounded in individual Git snapshots.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT">
  <img src="https://img.shields.io/badge/storage-Markdown-222222" alt="Knowledge stored in Markdown">
  <img src="https://img.shields.io/badge/interface-JSON-4c8c51" alt="JSON command-line interface">
</p>

<p align="center">
  <a href="#why-letopis">Why Letopis</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#knowledge-format">Knowledge format</a> ·
  <a href="#cli">CLI</a> ·
  <a href="#semantic-search">Semantic search</a>
</p>

## Why Letopis?

A new agent session often means rediscovering the same code, constraints, and decisions. Letopis gives agents working in one repository a shared memory they can maintain as they work and revisit on the next task.

| What you need | What Letopis gives you |
| --- | --- |
| Keep the reasoning behind a decision | Readable Markdown you edit, review, and version with Git. |
| Find context for the next task | Local full-text search, with optional semantic search. |
| Follow an idea back to its evidence | Explicit links between concepts, source files, and line ranges. |
| Keep knowledge grounded in different code versions | Each knowledge element can record its own Git source snapshot. |
| Notice when evidence changes | Source-change flags that tell an agent what needs review. |
| Use knowledge from scripts and agents | JSON requests and responses, with paths and hashes for reading the original content. |

**Write → check → search → follow links → verify sources.** Your files remain the source of truth; Letopis builds the search and connections around them.

## Installation

Install with [Homebrew](https://brew.sh):

```sh
brew install zdkadiev/tap/letopis
```

Native packages are available for macOS and Linux on ARM64 and x86_64. No Rust toolchain or model provider is needed to use full-text search.

<details>
<summary><strong>Build from source</strong></summary>

Build from source on macOS or Linux. You'll need rustup, a C compiler, platform build tools, and Make.

```sh
git clone https://github.com/zdkadiev/letopis.git
cd letopis
cargo fetch --locked
make release
```

The pinned Rust toolchain is selected automatically. Put `target/dist/letopis` on your `PATH`.

</details>

## Quick start

**1. Initialize your project.** Start at the root of the Git working tree where you want to keep knowledge:

```sh
printf '%s\n' '{}' | letopis init
mkdir -p letopis
```

**2. Save a concept.** Create `letopis/retry-budget.md`:

```markdown
<!-- letopis: {"created_at":1793491200} -->
# Retry budget

Keep retries bounded so a failing dependency cannot hold a request indefinitely.
Record the attempt limit and deadline when choosing a retry policy.
```

Use the current Unix timestamp for `created_at`, then preserve it on later edits.

**3. Check and find it.**

```sh
printf '%s\n' '{}' | letopis check
printf '%s\n' '{"query":"retry","limit":10}' | letopis search
```

Search returns links to knowledge, ready for the agent to read:

```json
{
  "ok": true,
  "result": [{
    "element_id": "letopis/retry-budget.md",
    "path": "letopis/retry-budget.md",
    "lines": [1, 5],
    "revision": 1,
    "flags": []
  }]
}
```

The line range and revision above are illustrative. Edit Markdown directly; the next search, check, traverse, or inspect refreshes the index automatically. No model provider is needed to get started.

## Knowledge format

Keep one concept per Markdown file. A metadata comment declares it as knowledge; its path becomes its ID. Connect concepts with ordinary inline links carrying an explicit relation:

```markdown
[Timeouts](timeouts.md "letopis:type=related_to;path=letopis/timeouts.md")
```

From `letopis/retry-budget.md`, this links to the marked file `letopis/timeouts.md`. Use `includes` to group concepts and `external` to connect a claim to source code:

```markdown
[Implementation](../src/client.rs#L10-L25 "letopis:type=external;path=src/client.rs#L10-L25")
```

The Markdown destination is relative to the document. The title's mandatory `path` is relative to the Git root. Adapt the example to your project's actual files.

**Knowledge can have different source bases.** One concept may have been verified against an earlier version of the code, while another was verified against a later version. Each element's optional `source_snapshot` records its own evidence basis; the collection does not have to share one source snapshot. Letopis compares bound sources with that basis to flag knowledge that needs review as code changes.

<details>
<summary><strong>File discovery, sections, and source verification</strong></summary>

Keep one concept per file. The `letopis/` folder is a convention; no folder registration is needed. Marked `.md` and `.markdown` files can live anywhere in the repository. Tracked files and untracked files allowed by Git ignore rules are discoverable; ignored untracked files are not. Ordinary Markdown without Letopis metadata creates no knowledge elements.

A whole-file element starts with its metadata comment on the first line. Its ID is its path relative to the Git root, such as `letopis/retry-budget.md`.

You can also mark a section in an existing document:

```markdown
## Retry budget {#retry-budget}
<!-- letopis: {"created_at":1793491200} -->
Keep retries bounded.
```

Its ID is the file path plus the explicit anchor, such as `notes.md#retry-budget`. Do not nest marked elements or mix a whole-file element with marked sections.

Source line ranges are inclusive. HTTP/HTTPS source addresses are also supported as references; their contents are not downloaded. Plain links create no graph edges. Relation names use lowercase letters, digits, and underscores, beginning with a letter. `includes` links must not form cycles.

To record the source version behind a claim, add optional `source_snapshot` metadata containing a complete Git tree OID to that element's metadata comment. For source matching committed `HEAD`, obtain a candidate with `git rev-parse 'HEAD^{tree}'` and verify the addressed bytes with `git show <tree>:<path>` before recording it. Preserve that element's basis until you have checked the claim against new source bytes. A changed source is a reason to review a claim, not proof that it is wrong.

Run `check` after editing. Fix invalid markup, missing knowledge targets, and containment cycles. Invalid files can retain their last valid indexed knowledge, so successful command execution alone does not mean every current file is valid.

</details>

## CLI

Commands work from the repository root **or any subdirectory**. Paths and element IDs in requests always remain relative to the Git root.

| Command | What you use it for |
| --- | --- |
| `init` | Set up Letopis in a project. |
| `check` | Find invalid knowledge, broken links, and source-basis problems. |
| `search` | Find relevant knowledge and items that need review. |
| `traverse` | Follow related concepts, collections, and source links. |
| `inspect` | Get hashes and a captured version for consistent reading. |
| `status` | See the current revision, problems, and embedding progress. |
| `config` | View your embedding configuration. |
| `reindex` | Explicitly rebuild the knowledge index. |

### Input → output

Supply **one JSON object on stdin**:

```sh
printf '%s\n' '{"query":"retry","limit":10}' | letopis search
```

Fields belong in JSON, not command-line options. Unknown fields are rejected. A terminal or empty input supplies `{}`; a pipe that sends no data within 250 ms also supplies `{}`. Once data arrives, Letopis reads until EOF.

Commands write **one JSON line to stdout**. Successful responses contain `ok:true` and a command-specific `result`, and may include a `warnings` array. Failed responses contain `ok:false` and an `error` with `code`, `message`, and `details`:

```json
{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}
```

Exit status is `0` for success and `1` for failure. Error codes are `invalid_request`, `not_found`, `validation_failed`, `embedding_failed`, `internal_error`, and `cancelled`. Warnings and validation findings use the same `code`, `message`, `details` structure.

`letopis --help`, `letopis <command> --help`, and `letopis --version` return plain text.

<details>
<summary><strong>Full command reference — request fields and response shapes</strong></summary>

### Initialize: `init`

Input: `{}`. Creates the local state and `.letopis/config.json`. Repeating it preserves existing valid configuration and data. It does not index knowledge or contact a model provider.

Output:

```json
{"ok":true,"result":{}}
```

### Validate: `check`

Input: `{}` to check the whole corpus, or `{"paths":["letopis/retry-budget.md"]}` to select findings for particular paths. An omitted or empty `paths` also audits complete source snapshots; a nonempty selection skips that full audit.

Output for valid knowledge:

```json
{"ok":true,"result":{"errors":[]}}
```

Inspect `result.errors` even when `ok` is true. Each finding contains `code`, `message`, and `details`; an empty array means no validation errors were found.

### Find knowledge: `search`

Input:

```json
{"query":"retry","limit":10}
```

Both fields are required; `limit` must be at least `1`. Optional `filters` accepts `needs_recheck` (a boolean), `sources` (source addresses), and `source_revisions` (complete Git tree OIDs). For example, find knowledge that needs review:

```json
{"query":"retry","limit":10,"filters":{"needs_recheck":true}}
```

Output is an array of matching links. This is an illustrative response; line ranges and revisions depend on your files:

```json
{"ok":true,"result":[{"element_id":"letopis/retry-budget.md","path":"letopis/retry-budget.md","lines":[1,5],"revision":1,"flags":[]}]}
```

Each link contains its ID, path, optional inclusive `[start,end]` lines, index revision, and freshness flags. Possible flags are `stale`, `source_changed`, and `source_not_found`; an empty array means no flag was reported. `needs_recheck:false` selects results without flags; omitting it includes both. Results contain metadata, not document text. An empty result does not prove that a fact is absent from the project.

### Follow connections: `traverse`

Input:

```json
{"element_ids":["letopis/retry-budget.md"],"mode":"both","direction":"both","depth":2,"limit":20,"edge_limit":40}
```

Required fields:

| Field | Meaning |
| --- | --- |
| `element_ids` | Nonempty array of starting element addresses. |
| `mode` | `relations`, `containment`, or `both`. |
| `direction` | `outgoing`, `incoming`, or `both`. |
| `depth` | Maximum number of hops; `0` returns no elements. |
| `limit` | Maximum returned elements; at least `1`. |

Optional `edge_types` restricts traversal to named link types. Optional `edge_limit` is at least `1` and requests edges as well as elements. Starting IDs are excluded from results.

Output when no connected elements are found:

```json
{"ok":true,"result":{"elements":[],"edges":[]}}
```

`elements` uses the same link objects as search. Each edge has `source`, `target`, and `type`. The `edges` field is omitted unless `edge_limit` was supplied.

### Read a consistent version: `inspect`

Input:

```json
{"element_ids":["letopis/retry-budget.md"]}
```

`element_ids` is required and nonempty. It can combine knowledge IDs with declared source addresses: file paths, inclusive `#Lstart-Lend` ranges, and HTTP/HTTPS addresses. An unknown ID fails the whole request.

Output contains `result.snapshot`, a retained Git tree OID, and `result.elements`, an object keyed by normalized requested IDs. Each value contains `path`, optional inclusive `lines`, and `hash` (SHA-256 of the addressed bytes, or `null` when unavailable). No text is returned.

Illustrative output, with placeholders for the tree OID and content hash:

```json
{"ok":true,"result":{"snapshot":"<git-tree-oid>","elements":{"letopis/retry-budget.md":{"path":"letopis/retry-budget.md","lines":[1,5],"hash":"<sha256>"}}}}
```

Use the returned snapshot to read the matching bytes:

```sh
git show <snapshot>:letopis/retry-budget.md
```

Replace `<snapshot>` with `result.snapshot`. For a line address, read its path from that same tree and select the returned range. This captures a consistent reading version during concurrent edits; it does not mean all returned knowledge was based on that version of the code. Each element retains its own declared `source_snapshot`. To examine its historical evidence, read the bound source from that tree with `git show <source_snapshot>:<path>`. Do not replace evidence bases with the inspect snapshot without checking the claims against its source bytes.

### Observe progress: `status`

Input: `{}`. Reports current state without refreshing knowledge. Example before the first publication:

```json
{"ok":true,"result":{"revision":null,"embedding_backlog":0,"embedding_failed":0,"locks":[],"problems":[]}}
```

`revision` is the last published index revision. The counters describe pending and failed embeddings. Held locks contain `name` (`index` or `embed`) and `pid`; problems contain `code`, `message`, and `details`. After fixing Markdown, run a refreshing command to update its validation findings.

### Rebuild: `reindex`

Input: `{}`. Reparses the corpus, audits effective source snapshots, and rebuilds the knowledge and full-text index. Example output:

```json
{"ok":true,"result":{"revision":2}}
```

Use it for a deliberate rebuild; routine edits only need the next refreshing command. It preserves declared source bases and does not wait for embeddings or confirm claims against changed sources. Run `check` afterward to review findings.

### View configuration: `config`

Input: `{}`. Returns the current configuration without changing it, or `{"ok":true,"result":null}` before setup. After initialization, the default response is:

```json
{"ok":true,"result":{"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0}}}
```

</details>

<details>
<summary><strong>Cancel a running command</strong></summary>

Send Ctrl-C (SIGINT) to your own running `search`, `traverse`, `inspect`, `check`, or `reindex` command. Cancellation returns the `cancelled` error shown above and exit status `1`. Work already published may remain; inspect status or retry. Background embedding work may finish independently.

</details>

## Semantic search

Full-text search works with embeddings disabled. To enable semantic search, edit `.letopis/config.json` directly. For an installed Ollama embedding model:

```json
{
  "embedding_model": {
    "enabled": true,
    "provider": "ollama",
    "model": "your-installed-embedding-model",
    "parameters": {"base_url": "http://127.0.0.1:11434/api"},
    "max_retries": 2
  }
}
```

Replace the model name with one available in your Ollama instance. OpenAI-compatible embedding endpoints are also supported. Letopis does not install models or start providers. Foreground operations may start background embedding work; use `status` to observe progress. `max_retries` counts retries after the initial attempt, so `0` permits one attempt. Setting `enabled:false` pauses embeddings while preserving existing vectors and retry state.

## License

[MIT](LICENSE).
