# TK-8: selected pedantic lint policy

User requested useful pedantic lints for learning Rust best practices. Added 17 explicit warning-level checks to `Cargo.toml`, retaining the existing `too_many_lines` check (18 selected pedantic rules total). The whole group remains disabled. Standard `clippy::all` and existing debug/stub prohibitions remain enabled.

The selected rules cover numeric conversions, ownership/cloning, control flow, documentation and Self-returning method results. [The development guide](../../../../../development.md#выбранные-pedantic-проверки) explains each rule and includes a checked conversion example. `AGENTS.md` requires concrete error/panic documentation and reasoned, narrow exceptions. Names/group membership were verified against the pinned Clippy 1.95.0 driver, and all-target Clippy recognizes the configuration.

The initial lint run found 15 unique missing public `# Errors` sections and one duplicated match arm (diagnostics were emitted twice for lib/lib-test targets). Added concrete error contracts to domain constructors/parsers, Markdown parsing, Git snapshot operations and the composition-root factory. Merged identical Markdown event arms without changing their action. No lint suppressions were added.

Verification on macOS ARM64:

- `./scripts/check-rust.sh`: formatting, compiler checks, all-target Clippy with `-D warnings`, and 53 root tests pass.
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`: passes, including public API links.
- All 18 selected rules are present in pinned Clippy's pedantic group; the entire group is not enabled.
- All 53 test names remain unchanged. No dependencies or test scenarios changed.

Evidence: `host.log`, `rustdoc.log`, `initial-findings.json`, `changes.diff`, and `verification.json`. Linux and historical isolated proof harnesses were not rerun for this lint-policy refinement. Previous receipts retain their original snapshot. Changes remain uncommitted in TK-8; no merge, push or GitHub issue mutation.
