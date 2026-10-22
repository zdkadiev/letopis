# Rust readability follow-up

Date:2026-10-22 (Europe/Moscow). Status: implemented, verified and independently reviewed in the TK-8 working copy; changes remain uncommitted.

## Accepted rules and implementation

- Tests use Arrange / Act / Assert. All53 original product-test names and order remain, with no missing/added tests. Both Git object formats and substantive error/GC/corruption/process scenarios remain covered. Sequential cases repeat phases; fixture checks are distinguished from behavior assertions.
- Functions/methods target about40 lines; Clippy too_many_lines enforces60 code lines with clippy.toml. Long Markdown scan, Git subprocess code and tests now use cohesive private helpers. Formatting was not compressed and blanket lint exceptions were not added.
- Meaningful Git kind/mode/version/ref-prefix values use private named constants and explicit imports. Domain owns the single production definitions for SHA-1/SHA-256 OID lengths, imported by the Git adapter. Independent expected values in tests remain oracles. No ObjectKind enum, new dependency/runtime or product subsystem was introduced.
- Rules are recorded in [AGENTS.md](../../../../../../AGENTS.md) and [development guide](../../../../../development.md). The Linux image receives the same clippy.toml.

## Verification

- Host `./scripts/check-rust.sh`: formatting, compiler checks, Clippy and53 tests PASS; no warnings/errors. [Log](logs/host.log).
- Source-built Linux ARM64/glibc runner `docker run --rm --network none letopis-tk8-check`:53 tests, fmt and Clippy PASS. [Build](logs/linux-build.log), [run](logs/linux-run.log). Final image:sha256:55f7d92d2cbc54a3afebf7eb75f214b4fe1f1237dc7d4db3d980192e48420716.
- Rust/Cargo1.95.0; host Git2.50.1 Apple, Linux Git2.39.5. All66 current source/config/check input hashes verified. Before/after test identities match53/53; [comparison](test-name-comparison.json).
- Independent whole-change review and scoped import-cleanup review completed with no open findings: [review](review.md). [Final working-copy delta](changes.diff), [size audit](function-sizes.json), [receipt](verification.json).

The automated60-line check excludes comments and blank lines; the40-line value is a readability target, not a mechanical compression rule. An unchanged Markdown links function remains64 physical lines but passes this code-line gate. Historical TK evidence remains pinned to its original snapshots and was not rewritten; nested proof-harness crates were not rerun. Prior user edits were in the review baseline and preserved; this report claims only the scoped readability refactor.
