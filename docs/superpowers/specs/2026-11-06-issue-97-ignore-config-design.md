# Discovery ignore configuration — issue #97

Date: 2026-11-06. Status: draft; user approval is pending. No product
implementation or implementation plan is authorized by this document yet.

Issue: [#97](https://github.com/zdkadiev/letopis/issues/97).
Branch and worktree: `isssue-97-ignore-config`.
Base: `main`, `52ed491a6688e0a4eee0cb5eceb3fc19f900465e`.

## Intent and accepted decisions

Give authors control over corpus membership without changing Git's tracked
membership or ignore files. Tracked generated Markdown can be excluded from
Letopis, and deliberately ignored local Markdown can become eligible.

The user selected the existing `gix` library and Gitignore semantics, including
the rule that a negated child cannot escape an excluded parent. They require
intact component boundaries, implementation mainly behind the Git interface,
and verification of every command, end-to-end workflows and varied patterns.

The selected library and semantics are accepted. The remaining behavior below
is a proposal until the user approves this design. In particular, the config
failure policy changes a currently tested fallback and requires an explicit
decision; it must not be inferred from the library choice.

## Current behavior and sources

- `src/git/worktree.rs` enumerates index membership, expands sparse-index
  directories privately, and enumerates non-ignored untracked paths with
  `gix_dir`. It returns raw NUL-delimited bytes, including deleted tracked paths.
- `GitWorktreePaths` is application-owned in `src/service/mod.rs` and implemented
  by `GitRepository` in `src/git/repository.rs`.
- `CandidateSelection` in `src/service/utils.rs` owns Markdown selection,
  private-component exclusion, scoped selection, final-symlink observations and
  decoding/normalization. This current ownership supersedes older historical
  descriptions of a `src/git/corpus.rs` component.
- `DocumentRefreshService` redetects changes under `index.lock`, atomically
  reconciles removed owners and derived projections, and rechecks excluded
  cached paths before publication. All refreshing commands use it.
- `Configuration` in `src/service/types.rs` currently models embedding state
  only. `EmbeddingConfig` hashes the configured provider projection separately.
  Configuration, initialization, status and embedding services interpret config;
  `LocalFileSystem` exposes generic file operations.
- `tests/knowledge_search.rs` explicitly permits FTS with missing, malformed JSON
  and unreadable config. Discovery configuration introduces a new decision about
  that fallback, rather than automatically inheriting the embedding policy.

Design sources: [architecture](../../zkit/letopis/architecture.md#program-structure),
[decisions](../../zkit/letopis/decisions.md#D-97),
[configuration decision](../../zkit/letopis/decisions.md#D-104),
[mechanics](../../zkit/letopis/mechanics.md),
[X-1](../../zkit/letopis/contracts/X-1-cli.md), and
[X-3](../../zkit/letopis/contracts/X-3-git.md).

## Approaches

1. **Selected by the user: reuse `gix`.** Use the already pinned parser and
   matcher behind `GitWorktreePaths`. Match each lexical ancestor as a directory
   before matching the leaf, preserving Git's excluded-parent rule. This needs
   a small amount of explicit integration and Git-oracle tests, with no new
   dependency or second filesystem walker.
2. **Not selected: add `ignore`.** A dedicated Gitignore builder/matcher is
   available, but adds another matching implementation alongside the existing
   Git stack. It still requires the same boundary and parity verification.

Product Git CLI calls, handwritten glob syntax, replacement candidate walkers,
new storage tables, an async runtime, nested Letopis ignore files, config mutation
commands and per-pattern diagnostic commands are outside this issue.

## Configuration contract

```json
{
  "embedding_model": {
    "enabled": false,
    "provider": null,
    "model": null,
    "parameters": {},
    "max_retries": 0
  },
  "discovery": {
    "respect_gitignore": true
  }
}
```

- The complete document still requires `embedding_model`; `discovery` is an
  optional object. An absent section or absent `respect_gitignore` member means
  `true`. A missing config file also means the default discovery policy.
- Only the two named root sections and the one named discovery member are
  accepted. Wrong types, null sections, duplicate keys and unknown fields fail
  strict full-document validation. Existing embedding validation and legacy
  migration diagnostics remain applicable.
- **Proposed toggle scope:** `false` disables all standard Git exclusions:
  per-directory `.gitignore`, common Git `info/exclude`, configured
  `core.excludesFile` and its default global ignore source. This corresponds to
  omitting `--exclude-standard` from the issue's independent Git oracle. `true`
  keeps the existing standard behavior.
- `config` returns the effective discovery section, including defaults, whenever
  a config file exists; its existing null result for a missing file remains.
  `init` includes the explicit default in newly generated config and preserves
  existing valid config bytes. No config-setting command is added.
- Discovery settings and ignore-file contents never enter embedding identity.
  Toggling them cannot trigger a model reset, reset retry budgets, clear query
  diagnostics or discard vectors for still-referenced hashes. Ordinary existing
  orphan cleanup for removed knowledge remains valid.

### Proposed failure policy — pending user decision

Corpus membership must be interpretable before refresh publishes anything.
Missing config defaults to standard Git ignores. Invalid discovery types,
unknown root fields, malformed JSON, duplicate keys and unsafe/unreadable config
fail discovery and leave the previous publication intact; no silent widening or
fallback to a previously cached policy is allowed.

A valid JSON root with valid discovery settings may still have invalid or
missing embedding settings. Discovery extracts and validates only its own
projection; refreshing FTS commands keep their current embedding-unavailable
behavior in this case. Full `config`/`init` validation continues to reject the
invalid embedding section, while `status` continues to report its existing
configuration problem. Share root/discovery parsing rules instead of adding a
second permissive JSON parser. Do not require provider validation to enumerate
the corpus.

This deliberately changes the existing FTS fallback for malformed JSON and
unreadable config. Update the old regression with explicit scenarios only if
this policy is accepted. Missing config and invalid provider settings must still
permit FTS. As an author-selected integration detail, malformed discovery input
uses `DocumentErrorKind::InvalidRequest` with a private config-validation cause;
existing consumer mappings produce the fatal `invalid_request` envelope with
their safe command context and empty details. Unsafe/unreadable config or ignore
file observations remain `internal_error`. `config` and `init` keep the current
`embedding_failed` envelope for full-config validation failure; `status` retains
`validation_failed` problems with `details.reason: "invalid_config"`. The issue's
`invalid_config` wording names the validation/status classification, not an
existing fatal CLI code: no such fatal code exists today. Do not add a new public
error code, fatal-error details mechanism or provider call for this feature.

## Ignore-file semantics

`.letopis/ignore` is one Gitignore-format byte buffer interpreted relative to
the repository root, despite its physical location inside `.letopis/`. No nested
Letopis ignore files or Git-side registration are involved.

- Apply it to the complete union of tracked and eligible untracked paths,
  including tracked paths covered by Git ignores and deleted tracked paths.
- Retain Git's blank lines, comments, escaping, trailing-space handling, CRLF,
  basename rules, root anchoring, slash rules, directory patterns, character
  classes, `?`, `*`, `**`, last-match ordering and `!` negation.
- A bare `name` can match that basename at any depth; `/name` is anchored at
  repository root. Root-relative does not mean every pattern is root-anchored.
- `notes/` plus `!notes/keep.md` excludes the child. `notes/*` plus
  `!notes/keep.md` can include that direct child because the parent is eligible.
  Re-inclusion through deeper directory patterns must satisfy each ancestor.
- Inherit Git's case handling from the repository's ignore configuration;
  explicitly test the relevant `core.ignoreCase` cases using the same Git oracle.
- Invalid/nonmatching pattern syntax follows Git/gix behavior. Do not add a
  stricter custom syntax validator or enable gix's non-Git precious-pattern
  extension.
- Missing file means no additional patterns. Empty and comment-only files are
  equivalent. `init` and indexing do not create it automatically.
- Read an existing file through generic confined no-follow filesystem methods.
  A symlink, directory, FIFO, unsafe parent, denied read or replacement race is
  an observation failure, never absence. Do not follow it or block reading a FIFO.
- Negation can restore only paths eligible in the Git candidate union. It cannot
  admit Git-ignored untracked files while standard exclusions are enabled, or
  override `.git`/`.letopis`, extension selection or existing path safety.
- Filtering uses raw root-relative path bytes before UTF-8/`RepoPath` parsing
  and document leaf probes. No filesystem traversal is needed for ancestor
  matching. Preserve current rejection of unsafe nonexcluded names and the
  exclusion of irrelevant raw names before decoding.

## Boundaries and integration

1. Application configuration parsing and discovery-policy values remain in
   `service/types.rs`; failures remain in `service/error.rs`. Keep the complete
   configuration distinct from its embedding state and identity projection.
2. The existing application candidate-selection path reads the config and ignore
   file through its injected `FileReader`. Application-owned constants name their
   paths. Generic filesystem APIs gain no Letopis-, discovery- or database-role
   methods. Reuse existing file guards, errors and bounded observation retry.
3. Extend `GitWorktreePaths::worktree_paths_get` with standard-exclusion mode and
   an additional root-relative ignore-pattern byte buffer. Use a finite enum for
   exclusion mode rather than an ambiguous boolean. Keep the signature in Git
   concepts, without config filenames, providers, Markdown or service roles.
4. `GitRepository` uses `gix_dir` with or without standard exclusions, then applies
   `gix` pattern matching to the union. No `gix` types escape `src/git/`; private
   pattern helpers stay there, under a responsibility-named file if needed.
   Index expansion, deleted tracked membership and repository safety stay intact.
5. `CandidateSelection` keeps application extension/private-path/scope and
   no-follow filtering. Domain data stays dependency-free. No forwarding service,
   new component implementation, dependency-bundling wrapper or extra generic
   injection parameter is needed.
6. `src/lib.rs` remains the composition root. Adjust only wiring and contracts
   needed by this behavior. All discovery consumers use the same selection path.

## Refresh, commands and concurrency

- `check`, `search`, `traverse`, `inspect`, ordinary `index`, `index --verify`,
  `index --full` and both flags together use the same effective corpus policy.
  Library incremental, verified and full refresh calls use it too.
- Removing an owner through either setting is the same cache/projection removal
  as losing candidate membership today: remove its `files` row, owned knowledge,
  outgoing projections and FTS contribution atomically; reconcile sources with
  existing rules. Incoming references remain governed by graph validation.
  `check` reports a remaining owner's dangling link to excluded knowledge.
- `inspect` of removed knowledge and traversal from a removed start return the
  existing `not_found` behavior. A surviving explicit external source binding to
  an excluded path still addresses source bytes: discovery policy selects
  knowledge owners, not source access permissions or historical snapshots.
- Scoped indexing removes only selected owners; unselected rows remain until an
  appropriate later refresh. `check.paths` scopes findings, not discovery.
- An effective membership change publishes a revision; changing policy bytes
  without changing the corpus does not by itself publish an incremental revision.
  Existing full-index revision behavior stays intact.
- `config` reads/validates configuration without discovery, refresh or provider
  calls. `status` reports the published state without refreshing. `init` creates
  no index publication. Internal `drain` consumes published work; it does not gain
  its own discovery/refresh phase. Help and version remain filesystem-independent.
- Re-read policy inputs during locked redetection; never freeze them at service
  construction or inside a shared Git clone. Existing long-lived service handles
  observe subsequent config and ignore edits.
- Carry the observed config and ignore contents for one discovery observation.
  Before a preliminary no-change return and before publication commits, reobserve
  those inputs safely. Changed contents/presence abort that observation with the
  existing documents-changed failure, preserving the prior publication. An edit
  detected after lock acquisition is interpreted afresh; a change during prepared
  publication must not mix policies. Keep existing excluded-candidate and document
  replacement checks. No new policy persistence, watcher or lock for author edits.
- Final rechecks do not promise atomic observation against arbitrary concurrent
  editor writes after the last check; a later command observes such edits under
  the existing live-filesystem concurrency contract.

## Verification and acceptance

Before implementation, the plan must enumerate concrete scenarios and independent
expected outcomes, including the following matrix. New/touched tests use fully
described Given/When/Then phases and receive manual mutation checks.

| Area | Required evidence |
| --- | --- |
| Git enumeration | Standard mode against `git ls-files -z --cached --others --exclude-standard`; disabled mode against the same command without `--exclude-standard`; tracked/untracked/deleted/staged/conflicted and sparse-index membership; both SHA-1 and SHA-256 |
| Pattern parity | Isolated scratch repositories with identical patterns in a root `.gitignore`, independent `git check-ignore --no-index` oracle and literal expected cases; basename/root/directory matches, ordering, negation, excluded parents, nested paths, glob forms, escaping, spaces, CRLF, comments, invalid patterns, Unicode/raw-byte names and case settings |
| Git exclusion sources | Root and nested `.gitignore`, `info/exclude`, explicit global ignore and default global ignore; tracked matches; both toggle directions; fresh observations through existing shared handles |
| Configuration | Old configs, absent file/section/member, true/false, wrong types/nulls, duplicate and unknown fields, malformed/unreadable file, valid discovery with invalid embedding, strict schema/runtime agreement, effective config output, no-clobber init |
| File safety | Missing/empty ignore, symlink/FIFO/directory, unsafe parent, permission failure where testable, replacement races, final-document symlinks/nonregular entries, invalid names, protected components and attempted private-root negation |
| Refresh lifecycle | Exclude/restore existing owners, newly included ignored owners, unchanged effective membership, same-stat policy edits, source reconciliation, dangling links, invalid markup becomes excluded, all index modes and scoped removal |
| Every command | Actual binary invocations for init/config/check/search/traverse/inspect/status/index and all supported index modes; help/version and internal drain; nested invocation directories; documented differences between refresh and observation commands |
| Embedding state | Synthetic local provider only; retained hashes keep identity/vectors/attempts/query-error state across discovery-only edits; no model reset or unexpected HTTP; excluded knowledge cannot leak into query results |
| Concurrency and rollback | Policy edits before locked redetection, during preparation and before commit; new exclusions as well as re-inclusions; early failure and late rollback; no-change recheck; preserve user index/HEAD/refs except normal Letopis-owned snapshot retention |
| Runtime boundary | Existing product executable-denial tests extended with configured discovery modes; no Git CLI/helper/filter/network fallback; architecture review of ownership, imports, physical files and composition |

Run `make verify` on the final changed tree and the schema-agreement Python check.
Run focused tests during development and keep independent expectations visible.
Record manual mutation failures and restored passing outcomes. Use fresh evidence
directories for an issue-specific command/pattern receipt and retain exact source
revision/diff, commands, versions, results and any platform exclusions. Exercise
Linux helper-denial tracing through existing infrastructure when available; report
unavailable platform checks instead of claiming them. The storage runner is only
required if storage/native/build/vendored inputs actually change.

Update current X-1 schema/examples/config fixtures, X-1/X-3 prose, D-97/D-104
clarifications, architecture/mechanics and user/developer/Letopis skill guidance
that describe discovery or exact config shape. Keep historical plans and captured
evidence against their original revisions. This design adds no performance target
or production/provider/power-loss acceptance claim.

## Remaining decisions and builder discretion

The user must settle the proposed config failure policy before this document can
be approved. Toggle scope, default config output and absent-ignore initialization
behavior above are explicit author proposals for the same design approval.

The builder may choose private helper organization, the exact finite exclusion
mode/type names, operation-scoped policy observation data, and focused test grouping
within the existing layout. They may not change accepted Git semantics, replace
gix, weaken confinement or invent a new component boundary to simplify the task.

### Builder check

An independent Rust builder review on 2026-11-06 identified one substantive
planning ambiguity: the fatal CLI representation of invalid configuration versus
the existing status-only `invalid_config` reason. The explicit mapping above
resolves it without a new public error envelope. The review raised no other
planning questions beyond the already pending user decision on failure policy.

## Sources and baseline

- [Gitignore reference](https://git-scm.com/docs/gitignore), especially pattern
  ordering, root-relative interpretation and excluded parent directories.
- [Pinned gix-ignore Search API](https://docs.rs/gix-ignore/0.24.1/gix_ignore/struct.Search.html).
  Installed gix 0.90.0 already reexports `ignore` and `glob`; no install was made.
- [Alternative ignore API](https://docs.rs/ignore/0.4.34/ignore/gitignore/struct.Gitignore.html).
- The fresh worktree completed `cargo test --locked --offline` with exit 0 before
  product changes. Library result: 591 passed, 0 failed, 9 ignored; root integration
  targets and doc-tests also passed. Ignored real-provider/corpus acceptance and
  historical harnesses were not independently rerun. This is a baseline, not
  evidence that issue #97 is implemented.
