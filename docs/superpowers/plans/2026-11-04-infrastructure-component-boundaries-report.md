# Infrastructure component boundaries report

The `rafactor` branch applies the agreed component boundaries and naming conventions to filesystem, Git, Markdown, process, embedding, CLI/signal and storage. Infrastructure operations use entity/action/filter names; application services retain use-case names. `AGENTS.md` is the canonical guidance, and `lib.rs` remains the composition root.

Filesystem interfaces use file and directory terminology, with one `LocalFileSystem` implementation. Git paths are selected for the knowledge corpus in application code; Git uses its in-process implementation. Markdown parsing keeps one parser. The HTTP provider keeps its two existing client pools under one owner. Detached process launching is generic; composition chooses its arguments. Storage retains SQL and transaction ownership while `StatusService` interprets status policy. Schemas and the intended public CLI behavior are preserved.

During integrated verification, Linux exposed a first-lock-creation race and a privilege-dependent permission-test assumption. The race was fixed with creation-scoped observation; strict observation remains for already observed ancestors. A separate review found that bytes-only configuration and embedding reads had acquired an unnecessary clock/mtime dependency; this was corrected with one shared safe read path. Both fixes received focused independent review. The initial failed Linux run remains available at `.artifacts/component-boundary-audit/task-8-linux-20261104T185848Z-514adce6/`; it is not represented as a pass.

Two independently reproduced issues remain outside this focused work, including a pre-existing database replacement race. Their evidence and limits are recorded in [the filesystem report](2026-11-04-filesystem-boundary-report.md). Earlier user-authored knowledge edits and existing commits were preserved.

## Verification

- macOS: `/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/component-boundary-audit/task-8-host-20261104-final-02` passed, including `make verify`; all 16 runner commands exited 0.
- Linux arm64: the same runner with `--platform linux` passed; both outer and container manifests report PASS, including `make verify`, native and standalone checks. Evidence: `.artifacts/component-boundary-audit/task-8-linux-final-20261104T201510Z-71cc7d71/`.
- Both runs used source commit `2b0ddd7` and 266 checked input hashes stayed unchanged. HEAD advanced only through documentation commits during the runs. Each runner used a controlled local provider for its approximately 60-second deadline test.
- Both Linux-only invalid-byte regressions passed in debug and release. Their independently restored behavioral mutation checks were recorded earlier on `15f7ba9`, not rerun on the final source.
- After source fixes, 11 affected knowledge elements and 70 source bindings were checked against tree `fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7`; one disposable `letopis check` returned `ok: true` with no errors.

Focused tests and restored mutation checks were recorded for the individual tasks; the original transient Task 5 observer logs were not recovered. Equivalent final-source observer checks have separate retained receipts. The earlier host acceptance on `4258be6` passed but predates the final filesystem fixes and is not final acceptance evidence. Only controlled local provider fixtures were used; no live or paid model runs were performed.
