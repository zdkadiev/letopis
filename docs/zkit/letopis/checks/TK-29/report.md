# TK-29 — cancellation, concurrent processes and maintenance workflow

Date: 2026-10-28. Base `e8e296de864d9b8c115a89235d7347083cca7b4b`
(`test: verify cancellation and drain isolation`). Worktree
`/Users/zdkadiev/.codex/worktrees/tk-29/letopis`, branch `tk-29`.

**Functional implementation delivered.** The maintenance CLI workflow and SIGINT
protocol are exercised end to end against the shipped executable on macOS arm64
and Linux aarch64. **Formal G-2 is not passed and the task card is `blocked`:** the
G-1/TK-17 prerequisite remains deferred (D-103), C-10 x86_64 remains TK-43 work,
and several requirements have historical rather than final-revision evidence. See
[g2-matrix.md](g2-matrix.md) for every individual row.

No product Rust, product dependency, schema or contract changed by Task 5 (since
`e8e296d`); Tasks 1–4 delivered the config command and cancellation path. The
final dirty tree contains only the new workflow test, the updated skill and this
report set.

## What changed

- `tests/maintenance_workflow.rs` — one shipped-binary scenario,
  `maintenance_roundtrip_preserves_knowledge_and_source_basis`, with private
  Given/When/Then helpers.
- `skills/letopis/SKILL.md` — complete config/status/reindex and SIGINT workflow.
- `docs/zkit/letopis/checks/TK-29/{report.md,verification.json,g2-matrix.md}`.
- No `tests/storage/Dockerfile`, `run.py` or `test_runner.py` change was needed:
  `tests/*.rs` and `tests/support` are already copied and hashed, and no compiled
  test reads `skills/letopis/SKILL.md`.

## Workflow scenario and observations

One real Git repository is created in a temporary directory. It contains a
committed source file, `src/cache.rs`, and two marked Markdown files:
`knowledge/decisions.md#cache` (with `depends_on` and `external` links and a
`source_snapshot` equal to the committed source tree) and
`knowledge/other.md#decision`. The source tree is independently verified with
`git show <tree>:src/cache.rs` before use. The test agent, never Letopis, writes
`.letopis/config.json`.

1. **Config absent.** `config` returns `{"ok":true,"result":null}`, and the
   `.letopis` directory remains absent.
2. **Valid local config, status, reindex.** The agent writes a local
   `ollama` configuration with `max_retries:0`; `config` returns the same value;
   `status` reports `revision:null`, zero counts and no problems; `reindex`
   returns revision 1; `status` then observes revision 1. The test seeds current
   vectors through the existing storage interface so the scenario does not depend
   on a background drain completing.
3. **Check, search, traverse, mixed inspect.** `check` returns `errors:[]`;
   `search` returns metadata for the knowledge element, not its text; `traverse`
   returns `knowledge/other.md#decision` and the source node
   `src/cache.rs#L2-L4` with the declared `depends_on` and `external` edges;
   mixed `inspect` returns one snapshot. The test runs
   `git show <snapshot>:knowledge/decisions.md` and
   `git show <snapshot>:src/cache.rs`, compares the knowledge bytes with the
   authored file, selects source lines 2–4, and independently computes both
   SHA-256 values with `sha2`. The product hashes match those values, and the
   declared source tree still contains the exact source bytes.
4. **Invalid config.** The agent replaces the config with an invalid file.
   `config` fails with exit 1, `embedding_failed`, `invalid embedding
   configuration`, and does not echo the canary. `status` still reports revision
   1 and adds `invalid_embedding_config`; `search` still returns the knowledge
   element through FTS. The agent then restores the valid file, `config` succeeds,
   `check` succeeds, `status` drops the config problem and `search` still works.
5. **Cancellation and retry.** A `check` process is started with the test Git
   proxy on `PATH`. The proxy acknowledges a live `ls-files` child over a Unix
   socket; no elapsed sleep is used. Only after that acknowledgement does the test
   send SIGINT. The caller returns exactly:

   ```json
   {"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}
   ```

   with exit 1 and no canary or request text. The test waits for the index lock to
   be free, proves the abandoned Git child is still alive, releases it, and proves
   it closes its observer socket. A fresh `check` then returns `errors:[]`.
   The authored Markdown, source file, Git index and HEAD are byte-identical to
   the capture taken before the first CLI command.
6. **Reindex remains explicit maintenance.** No command in the scenario uses a
   hidden config setter or a cancel subcommand. The only config changes are file
   edits made by the test agent. The only full rebuild is the explicit `reindex`.

The cancellation/commit distinction is preserved: a `cancelled` response promises
caller termination, not rollback. A publication that committed before process
death can survive. The scenario asserts that the retry is safe and that authored
knowledge and Git user state were never edited; it does not claim that cancellation
undid a committed publication.

## Focused evidence

| Command | Result |
| --- | --- |
| `cargo test --locked --offline --test maintenance_workflow -- --nocapture` | PASS, **1 passed / 0 failed / 0 ignored**, 18 raw envelopes in `.artifacts/tk29/focused-final-20261028T022000/maintenance.log` (SHA-256 `5a763be7c2907a0f37bcb4aee03e3a3bc5c886596ca6c8560302a12602354b50`). |
| Host-side schema validation: `python3 -B` using `tests/cli/run.py::validate_response` + `Draft202012Validator` + `docs/zkit/letopis/contracts/schemas/X-1-cli.yaml` | **18/18 envelopes accepted** (4 check, 4 config, 1 inspect, 1 reindex, 3 search, 4 status, 1 traverse). Log `.artifacts/tk29/focused-final-20261028T022000/schema-validation.log`. |
| Linux focused run in the storage image, `cargo test --locked --offline --test maintenance_workflow -- --nocapture` | PASS, **1 passed / 0 failed**, `.artifacts/tk29/linux-focused-20261028T021000/maintenance.log` (SHA-256 `fcbae5af19693cc54299477eaebaf9318c8588499f43fb1cf7c7510fda4f23b0`). 18 Linux envelopes also schema-validated, log `.artifacts/tk29/focused-20261028T014437/schema-validation-linux.log`. |

The compiled test has no Python or `jsonschema` dependency for schema validation;
the Git proxy fixture still requires `python3`. Envelopes are captured by printing
`WORKFLOW_ENVELOPE {json}` and the host validation is a separate step, as required
by the storage container's missing `jsonschema` module.

## Mutation control

Accepted mutation: `src/service/status.rs`, parse-failure branch of
`read_config_policy` changed from returning `invalid_config_problem()` to returning
no problem. The workflow test failed at the intended assertion:
`assert_problem_reason(status, "invalid_embedding_config", true)` failed with
`problems:[]` after the invalid config step. The file was restored byte-for-byte;
SHA-256 before/after `3e1deeb816b903e4bc25db1414c6ae14d1bdb94650f264b055b7fbe381a70272`.
Logs: `.artifacts/tk29/focused-20261028T014437/mutation-status-invalid-config.log`.

An earlier attempted CLI-dispatch mutation (`reindex` → `status`) was rejected at
compile time by the denied `dead_code` lint (`Request::Reindex` became unused), so
it is not counted as a behavioral control. Product source was restored before the
accepted mutation; `git diff` for `src/` is empty after both.

## Final platform runs

| Platform | Evidence | Result |
| --- | --- | --- |
| macOS 26.6.2 arm64 (host) | `.artifacts/tk29/host-20261028T015116` | PASS. `make verify`: dev **843 passed / 0 failed / 10 ignored**, release **843 / 0 / 10**. Manifest SHA-256 `e5cdbee5f588da3469292fc1a7e2b602f6f9116400bf7edbe820810fbfe45e25`. |
| Linux aarch64, glibc 2.36, Docker image `sha256:cb886e3f0ae592bb6f961754d5bd0bcc4dd34bf0f5a3787419940f49a32d4e30` | `.artifacts/tk29/linux-20261028T021500` | PASS. Inner `make verify`: dev **843 / 0 / 10**, release **843 / 0 / 10**. Inner manifest SHA-256 `ea350deaf0577e3e29f5c4da5165f0d2b329342640efb4a122b09a206f6a5b04`. |

Both runners record source commit `e8e296d…` with only
`M skills/letopis/SKILL.md` and `?? tests/maintenance_workflow.rs` dirty. Input
hashes: 358 host, 240 Linux (the Docker build context intentionally omits host-only
files). Host and Linux runners include `tests/maintenance_workflow.rs` in dev and
release profiles.

### Linux repeatability limitation

Two earlier fresh Linux runs failed **outside this task's scenario**:
`tests/cli_runtime.rs::malformed_requests_fail_before_service_directory_creation`
panicked with `BrokenPipe` in `tests/support/cli.rs:92` while writing input to a
product process that had already rejected invalid argv. The same test passes alone
in the same image (`.artifacts/tk29/linux-cli-runtime-20261028T021100/cli_runtime.log`:
1 passed, 0 failed), and the third fresh full run passed. This is a pre-existing
parallel-test race in the TK-11 fixture, not caused by product changes in this
task. Receipts are retained, not overwritten:

- `.artifacts/tk29/linux-20261028T015700` — full runner FAIL, exact trace.
- `.artifacts/tk29/linux-20261028T020300` — full runner FAIL, same trace.
- `.artifacts/tk29/linux-20261028T021000` — focused maintenance run PASS.
- `.artifacts/tk29/linux-20261028T021500` — full runner PASS.

No stable Linux repeatability claim is made. The final Linux PASS is actual
execution evidence, not an extrapolation; the two failures are retained as the
honest limitation.

## Skill update

The skill now documents:

- `config` absent/valid/invalid, read-only observation, no setter, no provider call
  or service-state creation;
- `status` revision/backlog/failed/locks/problems and its non-refreshing boundary;
- `reindex` as explicit maintenance, not the ordinary edit path;
- SIGINT of the caller: one cancelled JSON, exit 1, no second JSON after a normal
  result, no cancel subcommand, no PID-based cancellation;
- a cancelled response promises caller termination, not rollback; a completed
  commit may survive; retry the idempotent command and observe status;
- an independent detached `embed --drain` lifetime, durable attempt reservation,
  no cancellation handler and no authored-file changes;
- removal of the obsolete TK-26–29 disclaimers and the false instruction not to
  claim handled `cancelled` JSON.

The skill does not claim that SIGINT edits knowledge, that a cancel command exists,
or that a config setter exists.

## Architecture review of the final change

- Final change is test/skill/docs only; no domain, service, adapter, CLI,
  `lib.rs` constructor or dependency boundary changed.
- The new test uses the public test-support composition and the shipped binary; it
  adds no production hook, test-only product flag or environment checkpoint.
- The positive cancellation barrier is the existing Task 3 Git proxy/Unix socket,
  not a timing guess.
- The schema validation reuses the existing X-1 catalogue and project validator;
  no handwritten success-field check is the only validation.
- Owned children and the Git wrapper have cleanup guards on every failure path.
- Product source is clean; mutations were restored byte-for-byte. See
  [verification.json](verification.json) for hashes.

The broader product structure (Tasks 1–4) is covered by the prior task reports
listed below; this report does not claim a new independent review of that code.

## Shipped binary versus private fixture coverage

- **Shipped `CARGO_BIN_EXE_letopis`:** maintenance workflow (all commands),
  `tests/cli_cancellation.rs` (stdin, blocking flock, live Git child and consumed
  local HTTP), `tests/cancellation_concurrency.rs` (foreign holder, waiter, drain,
  late text). Both final runners execute these.
- **Private/in-process only:** response-owner races and storage commit/SQL
  cancellation in `src/cli/response/tests.rs` and
  `src/storage/cancellation_tests.rs`. They are not relabeled as shipped-binary
  evidence.
- The production `BackgroundRunner` detached-spawn path is historical TK-27
  evidence; `foreground_cancel_preserves_detached_drain` starts `embed --drain`
  directly, so this task does not re-exercise that spawn path.
- The workflow uses a local HTTP fixture and synthetic credentials only. No paid
  provider, pilot, real model request or model-quality assessment was performed.

## Historical receipts

Prior reports remain evidence at their captured revisions; changed paths make them
explicitly historical for this matrix. Report commits below are the last commits
touching each report directory, not the product revision asserted inside them.

| Task | Report | Report commit |
| --- | --- | --- |
| TK-1 | [report](../TK-1/report.md) | `ea86ead` |
| TK-2 | [report](../TK-2/report.md) | `1e795d7` |
| TK-3 | [report](../TK-3/report.md) | `c8f414b` |
| TK-4 | [report](../TK-4/report.md) | `6fd2d18` |
| TK-6 | [report](../TK-6/report.md) | `48b8ed8` |
| TK-8 | [report](../TK-8/report.md) | `aa5367d` |
| TK-9 | [report](../TK-9/report.md) | `360bbf7` |
| TK-10 | [report](../TK-10/report.md) | `12c5ad0` |
| TK-11 | [report](../TK-11/report.md) | `65c2602` |
| TK-12 | [report](../TK-12/report.md) | `8db4814` |
| TK-13 | [report](../TK-13/report.md) | `06dc8a1` |
| TK-14 | [report](../TK-14/report.md) | `c8ede9e` |
| TK-18 | [report](../TK-18/report.md) | `761d3b9` |
| TK-19 | [report](../TK-19/report.md) | `15ccc70` |
| TK-20 | [report](../TK-20/report.md) | `ed3e924` |
| TK-21 | [report](../TK-21/report.md) | `46456a0` |
| TK-22 | [report](../TK-22/report.md) | `3b27c95` |
| TK-23 | [report](../TK-23/report.md) | `026254a` |
| TK-24 | [report](../TK-24/report.md) | `d6657e8` |
| TK-25 | [report](../TK-25/report.md) | `e8eb1a2` |
| TK-26 | [report](../TK-26/report.md) | `8bc4fca` |
| TK-27 | [report](../TK-27/report.md) | `f007c88` |
| TK-28 | [report](../TK-28-status-acceptance/report.md) | `5a420e2` |

TK-29 Tasks 1–4 have no separate committed check report in this directory; their
implementation commits are `1584092` (config), `b68dc1e` (SIGINT ownership),
`269e89f`/`59ad8e8` (wait/commit boundaries) and `e8e296d` (drain isolation).
Their task reports live in the Superpowers session record, not as a product gate
report; this report only consumes the resulting tests and commits.

## Unrun and unverified

- G-1 minimum-hardware and TK-17 pilot: deferred by the user (D-103); no report.
- x86_64 macOS/Linux: not run; C-10 remains the TK-43 exception.
- Physical power loss/crash (TK-38), corruption/Git change (TK-39), backup/restore
  (TK-40), provider/offline drill (TK-41), exhaustion/trust boundary (TK-42).
- TK-30…TK-45 pilot, system validation, delivery and final acceptance.
- Paid provider calls, real model quality, system load campaign and stable Linux
  repeatability.
- The two earlier Linux full-run failures above remain open as a fixture
  repeatability concern; this task did not change that fixture.

## Final-review fix wave

After acceptance commit `a0b7614`, the whole-branch review found one test-watchdog
gap and a set of report/skill wording issues. The fixes are: the owned SIGINT test
child is now waited on with a 10-second deadline, then killed, reaped and reported
with its captured output; and the wording above is scoped to Task 5, cites the
existing Linux schema-validation and isolated `cli_runtime` logs, and corrects the
skill's corruption and problem statements. These changes are test and documentation
only. A scoped re-review of the fix wave was not performed; G-2 remains **not
passed**.
