# TK-15 — Reproducible target-scale corpus

> Status: approved by user “Go on with implementation”, 2026-10-24; implementation plan review pending.
> Base: main@c8ede9efa4a298d12651c2f5a206ab71fc60bed6.
> Worktree: /Users/zdkadiev/.codex/worktrees/tk-15/letopis; branch tk-15.

## Intent and sources

Prepare a repeatable, isolated corpus for the resource checks in TK-16 and later
queue/disk checks. TK-14 establishes the small real-CLI route; this task scales
its inputs and obtains real current embeddings. It does not establish RAM fit,
model quality, search latency, or memory usefulness.

Authoritative sources:
- [TK-15 card](../../zkit/letopis/roadmap/epics/EP-7-early-validation/tasks/TK-15-target-corpus.md).
- [Resource assumptions A-1/A-2/A-5 and D-90 states](../../zkit/letopis/resource-math.md).
- [Decisions D-90/D-93…D-96](../../zkit/letopis/decisions.md).
- [Architecture](../../zkit/letopis/architecture.md#program-structure) and
  [review checklist](../../zkit/letopis/architecture.md#program-structure-review).
- [Markdown X-4](../../zkit/letopis/contracts/X-4-markdown.md),
  [embedding X-2](../../zkit/letopis/contracts/X-2-embeddings.md),
  [CLI X-1](../../zkit/letopis/contracts/X-1-cli.md).
- [Existing loader](../../../tests/cli/README.md), [agent rules](../../../AGENTS.md).

## Accepted constraints versus proposed choices

Already required by the roadmap: 100,000 knowledge nodes, assumed 100,000 distinct
normalized source addresses, 500,000 distinct edges; 10–50 knowledge elements per
Markdown file; valid targets and acyclic includes. Full loading and all embedding
writes must pass through the existing product CLI/executor. Source nodes have no
embedding work. Metadata, source trees and hashes obey current contracts.

Author proposals accepted by the instruction to proceed, with the user’s later
model-routing correction preserved:
- Python standard-library tooling under tests/target_corpus, plus a small root
  Rust integration-test observer using existing public storage interfaces.
- A-2 expected profile, approximately 2,000 UTF-8 bytes per marked knowledge
  fragment, and A-5 30 elements/file (last file 10): 3,334 Markdown files.
  Record exact actual sizes; 2,000 is a fixture target, never a product limit.
- Existing local Ollama model embeddinggemma:300m-qat-q4_0 at explicit loopback
  API base URL http://127.0.0.1:11434/api; no remote paid provider or model download.
  Endpoint prefix clarified from the existing build_endpoint implementation: it
  appends /embed to the configured prefix. The model is already installed locally.
  Freeze and record the actual model
  digest, provider/config identity and observed dimension, not a guessed identity.
- User-selected model split: stronger models own generator design/implementation,
  the Rust observer, difficult fixes and final review. Use gpt-6-luna with medium
  reasoning only for bounded light tasks such as documentation and routine checks.
  Give subagents clean contexts and return concise summaries; keep logs in files.
  This replaces the initial proposal to assign main implementation to Luna.

## Corpus generation

Generate into a new explicitly named disposable directory, separate from the
project and pilot repositories. Refuse existing output instead of deleting or
reusing it silently. Fixed seed, stable ordering, fixed creation timestamps and
explicit generation parameters make bytes repeatable. Record tool versions and
source revision separately from deterministic corpus inputs.

Create and commit source files before rendering the knowledge Markdown so every
external link carries a real verified source tree. A source means a normalized
address, not necessarily a separate file. Shard distinct valid line addresses
among modest-sized source files; do not require 100,000 filesystem entries.
Exact source layout and graph algorithm are builder choices, recorded in manifest.

Use section elements with explicit unique anchors, adjacent metadata and no
nested marked elements. Include meaningful varied synthetic text with stable
identifiers; avoid identical search texts collapsing the intended embedding work.
A-2 statistics cover exact marked fragment UTF-8 bytes; separately report total
file bytes and searchable-text statistics where observable. Short stub payloads
do not satisfy the selected size profile. Keep synthetic content out of usefulness
pilot input.

Produce exactly the required unique node/edge totals. Use one distinct external
address per knowledge and deterministic internal links. Includes form a bounded-
depth DAG; remaining edges use existing ordinary relation types. Reject self-links,
dangling targets, duplicate triples, invalid ranges and incorrect totals. No new
types or product graph semantics are introduced. Independent tests use literal
small examples and malformed fixtures, not the generator as its own oracle.

## Loading and two states

Use actual built letopis binary and argv/stdin JSON. Initial search loads corpus
through change detection and projection publication as TK-14 does. No direct SQL
seed, fake vectors, library refresher, alternate import service or runtime.

Capture backlog before draining to completion: exact knowledge/source/edge counts,
current coverage, missing hashes, eligible work and exhausted/failed texts. Configure
the chosen model once; do not change configuration between compared states. A
controlled scheduling mechanism in test tooling may hold the owned drain lock
while the initial foreground refresh exits, then release it for real execution.
The builder must verify actual lock ownership and clean up only owned resources;
no new CLI switch or product test hook is assumed. Report a distinct unconfigured
loading snapshot if it is used; it cannot substitute for configured backlog.

Run production embed --drain with the real local model to obtain populated state.
No fixed completion deadline is a product requirement. Record duration/progress
and safe exit/attempt outcomes. Permit explicit bounded execution/interruption and
resumption without regenerating or rewriting cache; incompleteness remains visible.
Never claim populated from queued=0, HTTP success, vector table size, FTS fallback,
or controlled mock-server results.

Populated requires 100,000 knowledge nodes covered by current vectors for their
own search_hash and the same selected config. Observe each current node/hash,
vector identity/dimension and failures in one pinned storage snapshot. Check source
nodes separately and exclude them from coverage. Record unique search hashes as
well as covered nodes, missing/eligible/exhausted/failed counts and any config
mismatch. Existing StorageRead supports these reads; no aggregate product API is
required. Read vectors one at a time; do not retain 100,000 vectors in memory.

The test-only observer requires existing database files and validates their schema
before using open_storage, which may initialize absent schemas. It must not seed,
reset, repair or write vector data. Missing files and incompatible identity are
errors, not empty success. Compile observation tooling before resource measurements.
If the existing opener cannot meet these preconditions without mutation, report
that precise limitation before adding any product API.

## Evidence and reproducibility

New evidence directories retain parameters/seed, deterministic input hashes,
source tree, selected model digest/config identity, binary/revision/platform,
exact subprocess commands/exits and safely bounded logs. Record per-phase counts,
actual UTF-8 sum/mean/min/max and coverage verdict. Never log credentials or raw
provider responses. Output concise progress and final summaries; keep verbose
logs in files instead of streaming every test into agent context.

A rerun from the same seed/config reproduces corpus bytes and structural counts;
provider vector bytes need not be byte-identical. Retain original receipts and
capture new state separately. Document commands for small smoke and full target,
resumption, and later TK-16 use. Generated corpora/databases/weights stay ignored.

## Verification, scope and completion

Start with small seeded fixtures, independent count/reference/byte assertions,
malformed input and missing/stale/failed-vector observations. Use TDD for tooling,
then meaningful manual semantic negative controls with exact source restoration.
Run the relevant Python tests and Rust observer checks; make verify on changed
root Rust/public documentation. Reuse one completed verification result instead
of repeating unchanged gates. Platform claims apply only to actually run targets.

After smoke, run the full production load and real drain. Target counts/input
integrity can pass even while populated remains not assessed, but TK-15 is not
fully done until its populated criterion is met. Report pending work and actual
wall time honestly; no implicit speed or readiness guarantee.

No planned product src/dependency/schema/build/vendor changes. Any demonstrated
product defect is investigated first and scoped explicitly. Preserve hexagonal
boundaries; observer is test tooling, CLI handlers and lib.rs wiring stay unchanged.
Do not add a service, query-builder layer, async runtime, cache, new CLI fields or
commands for this task. Rust code follows project rules and rust-skills.

Not in scope: TK-16 RAM measurement/minimum-machine proof, TK-17 agent pilot,
latency guarantees, a full design audit, source lifecycle extensions, power-loss
claims, or automatically merging/pushing/closing the issue. Commit task documents
as required by Superpowers; later integration needs the user's instruction.

## Builder check

Fresh Luna/medium reviewer: nothing important left. No implementation changes.
Unchanged branch baseline `make check` exit 0; receipt under
`.artifacts/tk15/preparation/baseline.log`.

## Approval and follow-up

User approved proceeding with implementation on 2026-10-24. Next: review the
implementation plan, then execute with the selected model split. GitHub #49
tracks parallel drain separately; TK-15 uses the current sequential executor.
