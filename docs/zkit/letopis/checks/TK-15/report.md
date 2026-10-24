# TK-15 — Target corpus and production population

> Status: in progress; full production loading passed; full real-model population pending.
> Date: 2026-10-24. Branch: `tk-15`, base `main@c8ede9e`.
> Worktree: `/Users/zdkadiev/.codex/worktrees/tk-15/letopis`.

## Scope and architecture

The [specification](../../../../superpowers/specs/2026-10-24-tk-15-target-corpus-design.md)
and [implementation plan](../../../../superpowers/plans/2026-10-24-tk-15-target-corpus.md)
define the work. New files have five responsibilities: deterministic input generation,
independent validation, production CLI orchestration, existing-store observation and
tests/documentation. Python tools live in `tests/target_corpus/`; the Rust integration
entry is `tests/target_corpus.rs`, with a private helper in `tests/support/target_corpus.rs`.
The helper uses existing public storage traits and one pinned read snapshot.

No product `src/`, dependency, schema, vendor or native build change is present.
CLI handlers still invoke services; services depend on application interfaces;
technology implementations and `src/lib.rs` composition remain unchanged. The test
observer exposes only its necessary parent-module function. The runner's narrow
read-only SQLite metadata precondition prevents a stale resumed load from invoking
product refresh before identity verification; it neither seeds nor repairs data.
No runtime, forwarding service or product API was introduced.

[Parallel drain #49](https://github.com/zaurkadiev/letopis/issues/49) remains separate.
This acceptance uses the existing synchronous sequential executor. Merge, push and
GitHub task closure are not part of this execution approval.

## Input and reproducibility

| Property | Observed value |
|---|---|
| Seed / A-2 profile | 15 / expected, exactly 2,000 UTF-8 bytes per complete marked fragment |
| Knowledge / distinct source addresses | 100,000 / 100,000 |
| Distinct edges / includes | 500,000 / 73,333 |
| Marked fragment count / byte sum | 100,000 / 200,000,000 |
| Fragment min / mean / max bytes | 2,000 / 2,000 / 2,000 |
| Markdown files / packing | 3,334; 30 elements per file, last file 10 |
| Source files | 1,000 shards of at most 100 addressed lines |
| Hashed source and Markdown inputs | 4,334 |
| Markdown file min / mean / max bytes | 20,000 / 59,988.0023995201 / 60,000 |

The independent checker re-reads bytes, hashes, source-tree blobs, valid source
ranges, unique identities and edges, targets, packing and acyclic includes. A source
address is not a separate file. This corpus is synthetic resource data, excluded
from usefulness pilots.

Initial generation took **1.655149 s**, independent validation **3.307968 s**.
These are input preparation measurements, not product throughput. Manifest SHA-256:
`2825e0e6168d8dedf4b9c0fa770d677533ebd4f55c9f24461f565966d9e5bd96`.

The final path-only fix `84cbdad` rejects lexical `..` before filesystem access in
generator, validator and runner. After that fix, the checker validated the existing
full input in **3.595073 s**. A new input-only full replica generated in **1.925678 s**
and validated in **3.662776 s**: manifest bytes and all **4,334** input hashes match.
Neither corpus content nor active drain/config/binaries was changed. Evidence:
`.artifacts/tk15/final-path-validation-01/receipt.json` and `target-final-repro-01/`.

## Production loading and model pin

Full loading invokes the actual release CLI `search`, with empty-query JSON stdin,
while the runner owns the real `embed.lock`. This prevents detached drain from
consuming the configured backlog before observation. No document vectors are seeded.
Full load phase: **95.556470 s**; actual search process: **91.450130 s**.

Pinned provider: local Ollama, `embeddinggemma:300m-qat-q4_0`, API prefix
`http://127.0.0.1:11434/api`, `max_retries=2` (at most three attempts per hash).
No download or paid inference provider was used. Installed model digest:
`101341d65c2ccbf23f16650b79d30b9fca94a45ffa09a9984c600157b81a58df`.
Config identity:
`0cbbd152c2bdaa72c3f1ed0f087ec6fb97f4752286b54e213901a6e217646bb5`.
Config byte SHA-256:
`71ad59c12c393f8baed1a13d678b3a21305a00798f7b8dcfe5ba74531ea3d1b1`.
The immutable corpus-parent `runner-pin.json` also pins manifest and both binaries.
Source revisions/hashes remain historical provenance rather than an equality gate
that would reject a documentation-only commit.

| Pinned observation | Configured backlog | Final populated state |
|---|---:|---:|
| Knowledge / sources / edges | 100,000 / 100,000 / 500,000 | Pending |
| Unique current knowledge hashes | 100,000 | Pending |
| Covered / missing knowledge | 0 / 100,000 | Pending |
| Eligible / failed / exhausted hashes | 100,000 / 0 / 0 | Pending |
| Invalid files | 0 | Pending |
| Revision / dimensions | 1 / null before first vector | Pending |
| Verdict | `not_assessed` | Pending |

Full real-model drain started **08:37:18 UTC**. Monitor-04 at **08:55:45 UTC**
observed **23,369 covered**, 76,631 missing, 768 dimensions, zero failed/exhausted;
this is a historical progress snapshot, not population acceptance. Full coverage
must be observed after the real process finishes; zero queued work alone cannot pass.

## Verification and review

All raw evidence paths below are worktree-local ignored artifacts. Historical
receipts remain against their captured sources; they are not rewritten by later fixes.

| Scope | Result / source checkpoint | Evidence under `.artifacts/tk15/` |
|---|---|---|
| Generator/checker | 22 tests passed after multi-donor packing and code-context fixes | `task1-20261024-01/`, `task1-20261024-round1/` |
| Rust observer | 21 passed / 1 ignored entry, also exercised by subprocess; fmt and targeted Clippy passed | `task2-20261024-01/` |
| Runner | 24 tests passed after resumed-load fix, including actual release CLI with controlled local HTTP; earlier combined 44-test checkpoint retained separately | `task3-final-20261024-01/`, task3 round1 evidence |
| Final path guards | 25 focused no-model tests passed; generator/validator and pure runner path test | `final-fix-20261024-01/` |
| Root Rust gate | `make verify V=1`, exit 0; 481 passed / 0 failed / 5 ignored separately in debug and release | `root-01/` |
| Real-model small smoke | 60 / 60 current vectors, 768 dimensions; actual drain 2.450110 s | `task3-real-load-20261024-02/`, `task3-real-drain-20261024-02/` |
| Full input / configured backlog | Passed; exact counts and bytes above | `full-prep-01/`, `target-01/`, `full-load-01/` |
| Full real-model population | Running; not assessed | `full-drain-01/`, `full-drain-launch-01/` |

Root verification includes format, compiler/all targets, Clippy with denied
warnings and strict Rustdoc. Sixteen nested filtered test summaries are separate
from the root totals. Its **146** Rust/build/test input hashes still match after
the Python-only fix, so that gate is reused. Root log SHA-256:
`c8656405f481c95c377d794f0cc83b0006e95605aca292d7432e95eafcabcfd3`.

Product binary SHA-256:
`3c8f3a343667cd1258c289c49370d55d2b0612af6517d398ccc8b867af0f183e`.
Observer binary SHA-256:
`8866aaba2302fe4608fbe7937e2e15e679c75999ce4a0b8bc8970fcabf963ec6`.
Both remain unchanged. `full-prep-01/frozen-inputs.json` preserves the nine
actual full-run-start hashes; `final-path-validation-01/current-inputs.json`
separately records the five changed Python tool/test hashes. The final fix cannot
be represented as if it preceded the already-running drain.

Focused manual semantic controls: initial generator 2, generator fixes 2,
observer 2, original runner 7, resumed-load fixes 3, final path guards 3.
Each recorded control has an intended behavioral RED result and exact-restored
GREEN; observer controls compile before failing. No full-crate mutation sweep
was run or implied. These are historical controls at their captured revisions.

All four task reviews passed after scoped fixes. The strongest-model whole-branch
code review at `bbdb447` found no Critical or Important issues. Its Minor lexical
traversal finding was fixed in `84cbdad`. Deterministic inventory sorting is accepted:
O(bytes + edges + files log files), with measured full validation around 3.3 s.
The other Minor finding is explicitly deferred: `--max-seconds` bounds the direct
child wait, but a descendant retaining an inherited pipe could delay readers and
receipts. The current product redirects detached streams; no workload failure was
established. README documents the limit; no generic process framework was added.
One scoped final fix/evidence review remains pending population and final documents.

## Handoff and limits

[Commands and resumption instructions](../../../../../tests/target_corpus/README.md)
cover isolated generation, build artifact discovery, load, drain and observation.
Keep `target-01/`, its `.letopis` stores/config, parent pin and all receipt directories
for TK-16. New evidence directories are exclusive-created; never overwrite old
receipts or reset this populated state to manufacture a fresh backlog.

This run is macOS 26.6.2 ARM64, Python 3.12 and the pinned Rust toolchain. No new
Linux run or historical native/proof harness run is claimed. Normal SQLite sidecar
management may occur during observation; logical corpus/cache data is not mutated.
No RAM budget, minimum 4 GB machine fit, search latency, model quality, memory
usefulness, physical power-loss, full source lifecycle or G-1 success is proven.
TK-16 measures RAM; TK-17 evaluates usefulness on different real-agent inputs.

## Completion pending

Finish the existing full drain, obtain final current-hash/config coverage, fill
final counts/durations and task criteria, and complete scoped review, Engram record
and byte-verified Superpowers execution archive. Incomplete coverage remains
`not_assessed`; the small smoke and green root suite do not replace full acceptance.
