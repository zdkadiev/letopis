# TK-15 — Target corpus and production population

> Status: completed; full acceptance and scoped final review passed.
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
| Knowledge / sources / edges | 100,000 / 100,000 / 500,000 | 100,000 / 100,000 / 500,000 |
| Unique current knowledge hashes | 100,000 | 100,000 |
| Covered / missing knowledge | 0 / 100,000 | 100,000 / 0 |
| Eligible / failed / exhausted hashes | 100,000 / 0 / 0 | 0 / 0 / 0 |
| Invalid files | 0 | 0 |
| Revision / dimensions | 1 / null before first vector | 1 / 768 |
| Verdict | `not_assessed` | `populated` |

Full real-model drain started **08:37:18 UTC** and finished successfully at about
**10:47:49 UTC**, without a watchdog, reset or second drain. Actual CLI process:
**7825.869103 s**; complete drain phase:
**7831.240799 s** (2 h 10 min 31 s); wrapper:
**7831.305460 s**. These are observed host elapsed times with
periodic read-only coverage snapshots, not an isolated throughput benchmark or SLA.

The completed phase and a fresh final-source `observe` phase both read the same
100,000 current knowledge vectors under the same config/model/binary pin, with
768 dimensions and all missing/eligible/failed/exhausted/invalid counts zero.
The final read-only observation took **5.115246 s**.
The wrapper/process/observation receipts have exit/success results; no vector
payload was emitted. `check-final.py` independently asserts literal target counts,
backlog equality before drain, full observation equality afterward, durable pins,
config bytes, 146 root hashes, nine final tool/binary hashes and all 4,334 input
hashes. Summary: `.artifacts/tk15/final-acceptance-01/summary.json`, SHA-256
`e1cd3d5c893cf576588ce91dd4407d623920052de24f53d9dd9ff537b33dca69`. Raw receipts are hashed in that summary.

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
| Full real-model population | Passed: 100,000 / 100,000 current vectors; missing/eligible/failed/exhausted/invalid zero | `full-drain-01/`, `full-drain-launch-01/` |

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
was run or implied. Controlled changes: **19 caught / 0 missed / 0 timeout / 0 unviable**,
across those separate scopes, not a statistical whole-tree mutation result. These
are historical controls at their captured revisions. An earlier observer control
that failed to compile was discarded and is outside this selected viable scope.

All four task reviews passed after scoped fixes. The strongest-model whole-branch
code review at `bbdb447` found no Critical or Important issues. Its Minor lexical
traversal finding was fixed in `84cbdad`. Deterministic inventory sorting is accepted:
O(bytes + edges + files log files), with measured full validation around 3.3 s.
The other Minor finding is explicitly deferred: `--max-seconds` bounds the direct
child wait, but a descendant retaining an inherited pipe could delay readers and
receipts. The current product redirects detached streams; no workload failure was
established. README documents the limit; no generic process framework was added.
Scoped final fix/evidence review approved: `.artifacts/tk15/final-review-acceptance.md`,
SHA-256 `6f49183774c0131eb71836ec4f1955258e4524782a729c2ba3e8462a4326a9be`. It independently verified
13 raw receipt hashes, 146 root hashes, nine final tool/binary hashes and both
4,334-file corpora. All 13 rulings match the ledger; the two omitted inline
entries were corrected during review. No Critical or Important findings remain;
the documented pipe-reader Minor is accepted as nonblocking.

## Controller rulings

The following is the exhaustive chronological ruling record from this plan’s
execution ledger, including inline rulings. Each retains its stated cost if wrong;
the reviewed code and receipts resolve the concrete acceptance claims separately.

- use base_url http://127.0.0.1:11434/api — current provider appends /embed to prefix — cost if wrong: real-model smoke fails, no populated claim.

- trim generated Task4 brief at Primary integration heading — extractor otherwise carries non-task tail into Task4 — cost if wrong: primary acceptance could be omitted; retained in plan and ledger instead.

- no full-model generation during early task checks — deterministic tests first, one final acceptance run — cost if wrong: late scale defects require scoped fixes.

- optional Rust observer helper belongs in tests/support/target_corpus.rs — current CLI Docker COPY/inventory includes support recursively but not new Python harness directory — cost if wrong: helper ownership lives outside the Python tooling directory, no behavior/scope changes. Observer root tests must not require generated Python files to compile/run. Actual TK15 runner records its own Python inputs.

- deterministic sorting is acceptable at current scope pending measured bottleneck — avoids premature optimization — cost if wrong: validation overhead for4334inputfiles.

- observer requires existing WAL journal mode in addition to recognized version — product open_storage configures journal mode before rejecting full-schema mismatch — avoids header changes to unsupported input — cost if wrong: a legitimate DELETE-mode database needs product initialization before observer; acceptance uses already-created CLI WAL stores. Covered by nonmutation tests.

- Task3 may use actual CLI empty search query for initial refresh — Task2 sample demonstrated configured storage identity without provider inference; avoids unnecessary query/model work — cost if wrong: product smoke fails before fullrun. Counts/real drain independently verified.

- reuse this gate only if fingerprint unchanged at final acceptance — runs independent Rust checks during Python work without repetition — cost if wrong: any later Rust/input change requires a fresh gate.

- generate full immutable inputs while Task3 review runs — only accepted Task1 generator used, no product loading/inference or mutable runner code — cost if wrong: generator artifacts require a new directory after any actual input-generator fix. Full load/drain still waits Task3 review.

- Task3 may add a narrow sqlite3 stdlib read-only metadata precondition for genuinely incomplete first initialization, alongside existing Rust observer gate for initialized states — distinguishes safe empty owned store from stale configured data without adding product API — cost if wrong: extra test-tool metadata coupling; public Rust observer remains authoritative for acceptance and coverage, with no direct SQL/vector writes. Product/generator/observer unchanged.

- start accepted full load/drain concurrently with Task4 documentation-only work — no second implementer or runtime input changes; frozen hashes record exact code/binaries — cost if wrong: any code change invalidates affected receipts and requires new appropriate acceptance. No docs-only rerun.

- whole-branch code review begins after all4taskgates while fullpopulation runs — codeinputs are frozen and review can catch issues early — cost if wrong: final completion verdict remains pending, require one scoped evidence followup afterfullrun rather than pretend prepopulation review proves100kcoverage.

- defer inherited-pipe reader-future behavior — actualfrozenletopisdetached processredirectsallstreams and no workloadfailure observed; arbitrary descendant-retained pipes outside current supportedroute — cost ifwrong: watchdog can be exceeded; documentlimitation clearly, no new processframework. Final scopedreview seesruling and mayreject.

## Handoff and limits

[Commands and resumption instructions](../../../../../tests/target_corpus/README.md)
cover isolated generation, build artifact discovery, load, drain and observation.
Keep `target-01/`, its `.letopis` stores/config, parent pin and all receipt directories
for TK-16. New evidence directories are exclusive-created; never overwrite old
receipts or reset this populated state to manufacture a fresh backlog.
`target-final-repro-01/` is an unloaded, byte-identical input-only replica available
for a separate fresh backlog reproduction in TK-16; it has no model vectors.

This run is macOS 26.6.2 ARM64, Python 3.12 and the pinned Rust toolchain. No new
Linux run or historical native/proof harness run is claimed. Normal SQLite sidecar
management may occur during observation; logical corpus/cache data is not mutated.
No RAM budget, minimum 4 GB machine fit, search latency, model quality, memory
usefulness, physical power-loss, full source lifecycle or G-1 success is proven.
TK-16 measures RAM; TK-17 evaluates usefulness on different real-agent inputs.

## Completion and retained handoff

All TK-15 acceptance criteria and review gates passed. The existing shared Engram
task `entity_22509d20e7184480a77f891f62d7b3e3` is resolved at completion revision
**221**, commit `e27f125277f5e9f10bcd3bf1b48fa23448e22f53`. Useful findings, limits,
touched artifacts, source/acceptance commits and evidence references are linked
and contained; existing task attributes were preserved.

The owned Superpowers scratch workspace was copied and all **28 files** verified
byte for byte before removing only that workspace. It retained **13 rulings**.
Archive: `.artifacts/tk15/sdd-complete-84cbdad/`; manifest `archive.json` SHA-256
`48e53598c99a659b010eb72cd0ca589b1bb53189a92e5fd00b39e905d74242c5`. Raw review reports, corpus, populated
stores, model/config/binary pins and receipt directories remain in the task
worktree for TK-16. No unrelated workspace was cleaned.

Branch `tk-15` is ready for the user's later integration instruction. This task
has not merged, pushed or closed its GitHub issue automatically.
