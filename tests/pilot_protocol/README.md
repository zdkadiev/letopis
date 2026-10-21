# TK-5: P-1 protocol checker

Read-only Python 3.11+ standard-library tooling for the [pilot protocol](../../docs/zkit/letopis/pilot/protocol.md). It validates **supplied** study/session records and hashed evidence, then reports correctness, repeated investigations, agreement opportunities/violations, normalized token accounting and the raw P-2.7 workload rows.

```sh
python3 -B tests/pilot_protocol/check.py \
  tests/pilot_protocol/examples/demonstration/study.json \
  tests/pilot_protocol/examples/demonstration/sessions \
  --evidence-root tests/pilot_protocol/examples/demonstration

python3 -B tests/pilot_protocol/run.py --evidence NEW_DIRECTORY

docker run --rm --init \
  -v "$PWD:/repo" -w /repo letopis-tk2:rust-1.95.0 \
  python3 -B tests/pilot_protocol/run.py --evidence tests/pilot_protocol/.build/linux-new
```

The evidence directory must be new. No models, provider clients, package installation or Rust compilation are involved. The existing Docker image supplies Python; this is not an embedding transport test.

The supplied example is **synthetic demonstration data**. Literal accounting totals are 480 baseline tokens and 462 memory tokens, including 12 auxiliary embedding input tokens; these numbers were authored as test inputs and are not measurements or a utility result. Cached/reasoning components are subsets of normalized input/output, not additional totals. No winner, significance or improvement target is produced. `agent_executions` always reports zero executions performed by this checker, even when the operator supplies records labelled `pilot`.

CLI exit 0 means structurally consistent supplied observations, including reportable failures/unknown usage. Exit 2 means invalid records/evidence and a JSON error. `status` distinguishes `demonstration_only`, `incomplete_data`, `correctness_not_preserved` and `descriptive_only`. Unknown token totals stay null alongside known subtotal and unknown request IDs. Missing whole session slots/request-inventory entries, reused sessions, wrong state trajectories, wrong check/opportunity IDs and corrupt/outside evidence are rejected. Failed/not_run records are retained. `correctness_checks` exposes planned/assessed/not_run coverage and unresolved IDs; a failed task can still have missing acceptance evidence, which makes `data_complete=false`. When failure and incompleteness coexist, `incomplete_data` takes status priority while `correctness_preserved=false` and failed outcomes stay visible. `summarize` consumes records already checked by `validate`; the CLI always validates first.

`run.py` runs all unit/CLI tests, the supplied early example, a generated final shape and five separate bad datasets. It checks exact failure reasons, preserves the generated datasets/logs and hashes the source inputs and evidence files. It also verifies roadmap consistency and that all 60 TK-1…TK-4 source hashes still match their committed PASS receipt; it does not re-execute those unchanged Rust suites.

Operator forms: [study](../../docs/zkit/letopis/pilot/templates/study.json), [session](../../docs/zkit/letopis/pilot/templates/session.json), [report](../../docs/zkit/letopis/pilot/templates/report.md). Placeholders and unrecorded evidence are deliberately unready. Freeze real tasks/configuration and prove process isolation at TK-17/TK-30, not by replacing synthetic labels in this example. Production memory CLI, raw exporter integrations, actual task selection and all real pilot runs remain later roadmap work.

Limits: checksums/metadata do not prove actual sandboxing, model identity, hidden requests absent from both inventories, semantic correctness, fact/violation annotations or prospective task selection. These require operator evidence and review. Literal locators are not parsed as trusted events. Evidence confinement is a read-only file-boundary check, not a security test against a hostile concurrent writer. Profile counts/timings remain raw supplied data, not measured capacity/RAM or performance claims.
