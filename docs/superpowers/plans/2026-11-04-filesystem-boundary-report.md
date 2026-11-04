# Filesystem boundary refactor report

The refactor is on branch `rafactor`. `LocalFileSystem` implements generic file, directory and lock interfaces; callers own database and configuration roles. `lib.rs` composes the filesystem and storage. Operations preserve confinement, identity checks and typed errors; configuration and embedding inputs that need only bytes no longer require a representable timestamp or a clock reading.

Focused review found and corrected a newly introduced first-creation race and a byte-only read regression. The first fix allows safe concurrent initial lock creation; the follow-up retains strict checks for already observed ancestors. Captured/indexed files retain checked timestamps and reject unrepresentable values rather than saturating them. The changes and platform evidence are summarized in [the combined report](2026-11-04-infrastructure-component-boundaries-report.md).

## Known issues outside the current scope

- Replacing `cache.db` between separate SQLite opens can produce a `CacheStorage` whose reader and writer refer to different files. A controlled reproduction on macOS returned read revision 22 and write revision 11, then reopened the replacement with revision 12. This gap existed before the refactor. Reproduction output is retained at `.artifacts/component-boundary-audit/known-issues/database-replacement-repro.log`; it is not fixed by this change.
- A target-directory observation can be discarded during creation; replacing that target before the subsequent walk may go undetected. An independent controlled reproduction confirmed this on `15f7ba9`. It predates the final directory fix, but its status before this refactor was not established. It is not fixed by this change. The proof-only source copies are in `.artifacts/component-boundary-audit/known-issues/directory-repro-{path,tests}.rs`; they include test instrumentation and are not product source.

The user requested a focused refactor; these findings are recorded for separate work. Historical evidence and user-authored knowledge edits remain preserved.

## Verification

- macOS: `/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/component-boundary-audit/task-8-host-20261104-final-02` passed, including `make verify`; all 16 runner commands exited 0.
- Linux arm64: the same runner with `--platform linux` passed; both outer and container manifests report PASS, including `make verify`, native and standalone checks. Evidence: `.artifacts/component-boundary-audit/task-8-linux-final-20261104T201510Z-71cc7d71/`.
- Both runs used source commit `2b0ddd7` and 266 checked input hashes stayed unchanged. HEAD advanced only through documentation commits during the runs. Each runner used a controlled local provider for its approximately 60-second deadline test.
- Both Linux-only invalid-byte regressions passed in debug and release. Their independently restored behavioral mutation checks were recorded earlier on `15f7ba9`, not rerun on the final source.
- After source fixes, 11 affected knowledge elements and 70 source bindings were checked against tree `fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7`; one disposable `letopis check` returned `ok: true` with no errors.
