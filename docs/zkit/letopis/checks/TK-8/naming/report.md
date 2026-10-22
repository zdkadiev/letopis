# TK-8: verb names for functions and methods

Accepted user instruction: “methods names must be verbs or verb forms”. The rule is recorded in `AGENTS.md` and [the development guide](../../../../../development.md#имена-функций-и-методов).

Custom functions and methods describe actions. Custom accessors use `get_`; predicates use an appropriate verb such as `is_` or `has_`. Established Rust constructors/conversions (`new`, `new_with_cause`, `as_str`, `to_hex`) and implemented trait names (`fmt`, `Error::source`, `Drop::drop`) retain their required/conventional forms.

Examples applied in the current root crate:

| Previous name | Current name |
|---|---|
| `ObjectDb::entries` | `ObjectDb::read_entries` |
| `ObjectDb::verified` | `ObjectDb::verify_object` |
| `GitProcess::command` | `GitProcess::build_command` |
| `Retainer::exact_ref` | `Retainer::has_exact_ref` |
| `SnapshotError::kind` / `message` | `get_kind` / `get_message` |
| `RetainedTree::oid` | `get_oid` |
| `ByteSpan`, `LineRange`: `start` / `end` | `get_start` / `get_end` |
| `Edge::source` / `target` / `edge_type` | `get_source` / `get_target` / `get_edge_type` |
| `kind_for` | `derive_element_kind` |

These accessor/function renames change the Rust API spelling; current root-crate call sites and test helpers were updated. Test names and scenarios remain unchanged. Existing trait contracts for creating, reading and retaining Git snapshots are unchanged.

Verification: `./scripts/check-rust.sh` passed on macOS ARM64: rustfmt, compiler checks for all targets, Clippy with `-D warnings`, and all 53 root tests. See `host.log`. `rename-audit.json` compares Rust tokens against the immediately preceding working copy: only allowed symbol renames and the matching Serde deserializer callback string changed; whitespace/comments/commas and import ordering were normalized. This comparison supplements the tests; it is not a formal proof of behavior.

Scope: 22 Rust files plus agent/development instructions. Historical proof harnesses, original task plans and captured evidence remain historical; this receipt records the subsequent naming amendment. Linux checks were not repeated for this naming-only change. No new dependencies, runtime behavior, commits, merge, push or GitHub mutation. Existing unrelated user edits were preserved.
