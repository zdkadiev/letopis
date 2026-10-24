# TK-15 Target Corpus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate and load a reproducible 100k-knowledge/100k-source/500k-edge corpus, then prove current-vector coverage with the real local model.

**Architecture:** Python stdlib tools create isolated inputs and invoke existing Git/CLI commands. A root integration-test observer reads existing storage through public interfaces. Product modules, schema, dependencies and synchronous sequential drain remain unchanged unless a demonstrated defect requires a recorded scoped ruling.

**Tech Stack:** Python 3.11+ stdlib; pinned Rust toolchain and existing rusqlite/serde_json test dependencies; Git; installed local Ollama embeddinggemma:300m-qat-q4_0.

**Spec:** [approved specification](../../superpowers/specs/2026-10-24-tk-15-target-corpus-design.md).

## Global Constraints

- Work in `/Users/zdkadiev/.codex/worktrees/tk-15/letopis`, branch `tk-15`; base `c8ede9efa4a298d12651c2f5a206ab71fc60bed6`. Preserve unrelated work.
- Full target: 100,000 knowledge nodes, 100,000 distinct normalized source addresses, 500,000 distinct edges; valid includes DAG, no dangling targets/self-links, 10–50 elements/file.
- Proposed/approved default: A-2 expected approximately 2,000 UTF-8 bytes per exact marked fragment, A-5 30 elements/file, last file 10. Record exact byte statistics and seed, never claim node count substitutes for payload size.
- Use real product CLI loading and embed --drain for acceptance; no SQL seed, fake vectors, library refresh, new CLI fields or product runtime. Source nodes produce no embedding work.
- One chosen provider/config throughout backlog and populated; installed local model, explicit loopback URL, no paid API or downloads. Record model digest/config identity/dimensions.
- Populated requires all current knowledge hashes covered under the pinned config; zero eligible work is insufficient. Missing/config-mismatched data fails observation. Exhausted failures remain missing.
- No fixed drain SLA; record progress, safe interruption/resumption and incomplete verdict. No RAM/latency/usefulness or minimum-machine claims.
- Stronger models own generator/observer/runner and reviews; Luna/medium only light docs/routine tasks. Clean contexts; no nested subagents, no parallel implementers; file-based reports/logs, concise summaries.
- Follow AGENTS.md and rust-skills; functions aim 40/max60 lines, focused files, idiomatic visibility/imports and Arrange/Act/Assert. No product src changes planned; #49 parallel drain remains separate.
- Tests use TDD and independent literals/oracles. Capture compiling/behavioral RED→restore→GREEN controls. One appropriate full verification per final source snapshot; no full cargo-mutants sweep.
- New evidence paths only; output creation refuses collisions; never overwrite, remove or signal unrelated data/processes. Keep generated outputs ignored. Merge/push/issue closure are not authorized by this plan.

## Review Focus

1. Same seed under a different parent path/global Git settings reproduces file bytes/tree; existing destinations and traversal/symlink paths are rejected without changing prior data (Task 1/3).
2. Multibyte UTF-8, remainder file and graph boundary nodes retain exact size/counts with valid unique source ranges/targets and includes without cycles (Task 1).
3. Empty/missing/incompatible stores, changed config, shared hashes and exhausted failures cannot masquerade as 100% coverage or initialize data during observation (Task 2).
4. Foreground background-spawn races cannot complete the corpus before backlog capture; exceptions release only the runner’s own lock/process resources (Task 3).
5. Interrupted runs preserve corpus/config identity and prior receipts; resumption neither resets valid vectors nor silently declares populated after provider failure (Task 3/4).

## File ownership and interfaces

| File | Responsibility / owner |
|---|---|
| `tests/target_corpus/generate.py` | Deterministic Markdown/source generation and manifest, Task 1 stronger worker |
| `tests/target_corpus/validate.py` | Independent generated-input structural/byte validation, Task 1 |
| `tests/target_corpus/test_generate.py` | Literal fixture/count/reference/UTF-8 tests, Task 1 |
| `tests/target_corpus.rs` | Root integration-test observer entry, behavioral checks, Task 2 stronger worker |
| `tests/support/target_corpus.rs` | Test-local storage observation implementation if needed to keep entry focused, Task 2 |
| `tests/target_corpus/run.py` | Product CLI orchestration, lock ownership, receipts/resumption, Task 3 stronger worker |
| `tests/target_corpus/test_run.py` | Runner/control and failure-path tests, Task 3 |
| `tests/target_corpus/README.md` | Reproducible commands, state/limits/handoff, Task 4 Luna |
| `docs/zkit/letopis/checks/TK-15/report.md` and task card | Actual results and limits, primary agent after real runs |

No future-subsystem scaffolding. If a file would exceed 500 lines, split one cohesive responsibility locally and ledger the interface; do not create general frameworks.

### Task 1: Deterministic corpus generator and independent input checks

**Files:** create `generate.py`, `validate.py`, `test_generate.py` above.

**Interfaces:** consumes current X-4 Markdown grammar and Git source-tree rules. Produces `generate_corpus(output: Path, options: CorpusOptions) -> dict` and `validate_corpus(manifest_path: Path) -> dict`. `CorpusOptions` is a dataclass with `seed: int`, `knowledge_count: int = 100000`, `fragment_bytes: int = 2000`, `elements_per_file: int = 30`. CLI: `python3 -B tests/target_corpus/generate.py --output PATH --nodes N --seed S [--fragment-bytes B] [--elements-per-file F]`. Default N=100000, S=15. `output/repository` contains the isolated Git fixture; `output/corpus.json` contains schema_version=1, options, source_tree, repository-relative input SHA256 map, knowledge/source/edge totals, file packing and exact fragment/file UTF-8 stats. Deterministic fields exclude absolute output path/timing; manifest resolves repository as sibling `repository`.

- [x] **Step 1: Write failing behavior tests.** Pin a 60-node smoke: 60 knowledge/60 source/300 unique edges, two Markdown files of 30. Assert stable tree/input bytes across two parent paths and hostile global Git configuration, UTF-8 literal byte lengths, includes DAG and independent literal first/last target identities. Assert 100000 packing yields 3334 files with last10 without materializing all payload in the unit test. Cover a valid small remainder distribution and reject impossible packing, existing outputs, symlinks, invalid counts, cycles, duplicate triples, out-of-range sources and a deliberately modified fragment/hash.
- [x] **Step 2: Run** `python3 -B -m unittest discover -s tests/target_corpus -p test_generate.py` into a new task log. Confirm RED because generator/validator behavior is missing, not malformed test setup.
- [x] **Step 3: Implement the generator and checker.** Fixed timestamps/seed, explicit UTF-8/newlines, isolate Git config/env. Commit source tree first. Use source shards of 100 addressed lines; one valid unique external source per knowledge. Generate bounded-depth within-file includes and ordinary links so each knowledge owns exactly five distinct edges; terminal nodes substitute an ordinary link. Choose ring offsets so no self/duplicate triple exists for supported small inputs. Size complete marked fragment near/exact target after metadata/links; record actual stats. `validate.py` re-reads bytes/references instead of trusting manifest counts, validates source tree contents and target ranges, and uses iterative DAG validation. Keep algorithms linear in input bytes/edges; do not retain vectors or giant concatenated corpus text.
- [x] **Step 4: Run tests GREEN and semantic controls.** Independently remove a counted edge and shorten a payload in disposable copies; checker/tests must reject each. Restore original bytes and verify GREEN. Store full evidence in files, return summary only.
- [x] **Step 5: Self-review and commit only owned files.** Report commit, exact commands/status and manifest interface. Primary dispatches a task-scoped independent review before Task 2.

### Task 2: Existing-store observer with hash/config-aware coverage

**Files:** create `tests/target_corpus.rs`, optionally private `tests/support/target_corpus.rs`.

**Interfaces:** consumes Task 1 manifest/repository. Exposes ignored test entry `observe_existing_corpus`, invoked from the compiled integration-test executable using `--ignored --exact observe_existing_corpus --nocapture`. Environment `LETOPIS_CORPUS_REQUEST` points to JSON `{root, output, expected_knowledge, expected_sources, expected_edges}`. Read config from that root. Output is exclusive-created JSON receipt with schema_version=1, project_revision, config_identity, dimensions, knowledge/source/edge counts, unique_search_hashes, covered_nodes, missing_nodes, eligible_hashes, failed_hashes, exhausted_hashes, invalid_files and verdict (`populated` or `not_assessed`). No vector payload is emitted.

- [x] **Step 1: Write failing observer tests** for current and missing coverage, sources excluded, shared-hash two-node coverage, exhausted failure with zero eligible work, config mismatch, absent/empty/wrong-version databases and existing output. Use small actual product-loaded fixtures where practical; handcrafted vector states are permitted solely in observer unit tests, never full acceptance. Assert known literal counts and that invalid observation does not create/change databases.
- [x] **Step 2: Run** `cargo test --locked --offline --test target_corpus` into a new log; observe meaningful RED before implementation.
- [x] **Step 3: Implement guarded observation.** Before `open_storage`, reject missing/non-regular/symlink files and reject version0/unsupported schema using an existing-file read-only rusqlite connection (`PRAGMA user_version`, version1 expected); then existing product opener verifies full schema, avoiding its empty-store initialization branch. Use `Storage::begin_read`, `StorageRead::{list_nodes,list_edges,list_files,get_revision,get_config_identity,read_vector_dimensions,read_vector,read_failure}` and `NodeMetadata::Knowledge.search_hash`. Parse config through `EmbeddingConfig`. Check config identity before coverage; inspect each unique hash once while preserving node coverage. Read one vector at a time and verify dimension/finite values. Separate failed/exhausted from eligible work by current attempt budget. Enumerate counts from actual persisted state, not input manifest alone. Refuse malformed output/input and identity mismatch instead of success. Keep observation reads within one pinned snapshot.
- [x] **Step 4: Run GREEN, fmt and targeted Clippy; perform controls.** Drop current-vector/hash check and suppress exhausted classification separately in disposable copies; relevant assertions must fail. Exact restore/GREEN. Audit no public product interface or schema changes.
- [x] **Step 5: Self-review and commit owned files.** Include compile/test evidence and exact receipt shape. Primary dispatches independent task review before runner implementation.

### Task 3: Production CLI runner, backlog capture and resumption

**Files:** create `tests/target_corpus/run.py`, `tests/target_corpus/test_run.py`; touch prior interfaces only with a recorded ruling and coordinated ownership.

**Interfaces:** consumes corpus.json schema_version1 and compiled product/observer binary paths. CLI: `python3 -B tests/target_corpus/run.py --corpus PATH/corpus.json --binary ABS --observer ABS --evidence NEW_DIR --phase load|drain|observe [--max-seconds N]`. `load` creates explicit local Ollama config once with model embeddinggemma:300m-qat-q4_0, parameters base_url=http://127.0.0.1:11434/api, max_retries=2 (at most three attempts per hash); verify actual provider endpoint construction before freezing. User-supplied config, if supported, must be explicit and captured safely; no silent defaults to a paid provider. Keep config outside generated-input hashes and pin its identity in evidence. `drain`/`observe` reuse existing config and require its match, never rewrite/reset it.

- [x] **Step 1: Write failing runner tests.** Verify argument/stdin handling through a real small CLI smoke, backlog barrier, existing-output refusal, config/model drift rejection, subprocess nonzero/timeout classification, immutable old receipts and resumption preserving existing vectors. Fake subprocesses may test orchestration failures; they cannot satisfy product/model acceptance. A blocked local HTTP fixture may exercise the lock race without loading a model in root tests.
- [x] **Step 2: Run** `python3 -B -m unittest discover -s tests/target_corpus -p test_run.py` into a new log and confirm expected RED.
- [x] **Step 3: Implement phases and receipts.** Validate corpus before load, inspect local Ollama installed model/digest without pulling, preserve Git env isolation. Hold an owned `fcntl.flock` on the disposable repo's stable `.letopis/embed.lock` during configured actual CLI `search` refresh and backlog observation; mode/identity must be compatible with product confinement. Check no drain owns the lock beforehand, release only own descriptor. Foreground query may infer one query but must not consume document backlog. Invoke observer receipt, verify expected counts and nonempty backlog. `drain` owns only its direct process/session, captures safe bounded logs/exit/duration, honors optional explicit watchdog (not product SLA), reaps owned child, then observes coverage even after bounded interruption/failure. Preserve per-phase counts, model digest, config identity, binary/source hashes/platform/commands and actual document coverage. `observe` never starts product drain or modifies config. Do not stream provider payloads, knowledge or 100k rows to agent output. Return nonzero for failed prerequisites and incomplete requested full population, with durable explicit not_assessed receipt.
- [x] **Step 4: Verify tests GREEN and a 60-node actual CLI smoke.** Use controlled HTTP only for deterministic integration controls; label it. Remove lock barrier and falsify coverage/config check in separate disposable controls; demonstrate intended failures, restore/GREEN. Prepare real-model small sample only after deterministic tests pass; record actual duration without extrapolating it into a guaranteed full-run SLA.
- [x] **Step 5: Self-review and commit owned files.** Primary reviews actual ownership/races, process cleanup and safe resume, with independent task reviewer. Do not launch full100k run until root verification and this review pass.

### Task 4: Light documentation and reproducible handoff

**Files:** create `tests/target_corpus/README.md`; update related command references if necessary, without editing product code or acceptance claims.

**Interfaces:** consumes finished CLI options and receipt schema from Tasks 1–3. Produces copyable build/smoke/target/backlog/drain/observe/resume commands and clear expected verdicts. Luna/medium owns this task; primary retains actual acceptance/report ownership.

- [x] **Step 1: Read implemented --help and task reports.** List actual build artifact discovery and phase prerequisites. Do not invent unsupported switches or claim pending measurements.
- [x] **Step 2: Write commands and safety/limits documentation.** Explain source addresses vs files, exact A2 bytes/seed, input/model/config hashes, source-vs-knowledge snapshots, read-only observer preconditions, backlog/populated receipts, local-model-only policy, timeout/resume/owned cleanup, generated-output location and TK16 handoff. Full corpus is excluded from usefulness pilots. #49 remains a separate sequential-to-parallel proposal.
- [x] **Step 3: Validate documentation.** Run documented --help and a disposable small smoke command where existing receipts do not already cover it; verify local links and diff whitespace. No rerun of unchanged root suites, no behavioral tests for prose alone.
- [x] **Step 4: Commit owned documentation.** Return concise status/commit/check summary. Primary independent task review checks commands against implemented interfaces.

## Primary integration and completion

- [x] Run all focused Python tests, `make verify` (logs to new evidence), and build/discover release observer/product artifacts. Record actual results once; inspect architecture/no product delta and compare frozen inputs before long run.
- [x] Generate full target under ignored `.artifacts/tk15/target-01`, validate actual source/input counts/bytes and reproducibility receipts. Capture configured backlog with the production release CLI. No SQL seed or model fixture counts as acceptance.
- [ ] Run real local-model drain, with concise durable progress and bounded observation intervals. Observe current-vector coverage through the release observer. If interrupted/incomplete, retain artifacts and resume; do not mark TK15 done until all criteria are supported. Report measured small/full duration, failures and platform scope.
- [ ] Fill `docs/zkit/letopis/checks/TK-15/report.md`, task card and this plan with actual evidence/limits. Preserve full-scale inputs/receipts separately from Git. Record Engram using the existing shared store and unchanged provenance.
- [ ] Dispatch one whole-branch review on the most capable available model with spec/plan/diff/evidence; resolve findings and record any declined scopes. Do not rerun accepted suites unless code changes warrant it.
- [ ] Archive byte-verified plan ledger/briefs/review artifacts before deleting only owned scratch workspace. Commit task completion docs, retain worktree/evidence, and report ready for the user's later merge/push instruction.

## Self-review

Spec coverage: generation/bytes/DAG Task1; current hash/config/failure coverage Task2;
production route/backlog/model/resume Task3; reproducible handoff Task4; real full
acceptance/report/final review primary. All five Review Focus entries have named
checks in owning tasks. File/interface/signature names match across tasks. Four
reviewable tasks, no product restructuring or #49 implementation. User approved this plan with “Go” on 2026-10-24; subagent-driven execution
is selected, with stronger implementation models and Luna on light tasks.
