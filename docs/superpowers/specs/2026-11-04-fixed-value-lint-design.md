# Fixed-value helper check — agreed direction

## Intent

Detect functions that exist only to construct a fixed error or fixed response, so callers construct the value explicitly. Repeated message text may be a named constant. Keep error conversions that use their inputs, real builders, and trait-required constructors.

## Scope and behavior

- Add a development-only Rust syntax check, using the already locked `syn` 2.0.119 parser as a direct development dependency with the necessary parsing and traversal features. It does not add a product runtime dependency.
- Scan Rust files under `src/`, `tests/` and `examples/` independently, including inline modules, inherent methods and `cfg(test)` items. Do not follow symlinks. Fail on an unreadable or unparseable source file instead of silently skipping it. Diagnostics are deterministic and identify the file and function.
- Check free functions and inherent methods with no parameters other than an unused receiver, whose complete body is a single expression (optionally wrapped in `return`, parentheses or braces). Functions with other parameters are outside this first check, even when a parameter is unused.
- Recognize a conservative allowlist of fixed expressions: `*Error::new` or `*Error::new_with_cause` with fixed arguments; `CommandResponse` struct literals whose fields are fixed; and `Ok`/`Err` wrappers around those expressions. Fixed arguments include literals, enum-variant paths, uppercase constant paths, `Vec::new()`, `String::new()`, and `std::io::Error::other` applied to a fixed value. This is a syntactic rule: `syn` cannot prove whether a path denotes a constant or whether an arbitrary call is pure. Fixed responses of other types, aliases, and unrecognized nested constructors are not detected in this first version.
- Flag, for example, a no-argument helper returning `SnapshotError::new(SnapshotErrorKind::InternalError, MESSAGE)` or `Err(CommandError::new(CommandErrorKind::Cancelled, MESSAGE))`, and a no-argument helper returning a fixed `CommandResponse`. Allow a named message constant at each explicit construction site.
- Exclude trait implementations and trait default methods, conventional `new`/`default` constructors, and generic functions and impls (including lifetime-only generics). Keep error mappings that use their inputs and builders that perform meaningful work. Do not classify arbitrary calls as pure; unsupported nested constructors, macros and multi-statement data-flow are deliberate false negatives for this first check.
- Parse syntax rather than search raw text, so comments and strings cannot masquerade as functions. Keep invalid checker examples as in-memory test strings or non-`.rs` fixtures, outside the scanned source set.
- Integrate the check and its tests into `make check`, so `make verify` includes them. Test positive and negative examples, nested `Err`, constant messages, nested fixed I/O causes, trait and constructor exemptions, and parse failures. Prove that a deliberate violation fails the check.

## Existing code

A read-only inventory found at least 17 fixed-error constructors in active source, with many callers. The user requested fixing all audit issues; replace each helper with explicit error construction at its call sites, preserving error kinds, causes, messages and lazy evaluation at call sites such as `ok_or_else`; share only meaningful repeated message constants. Do not rewrite unrelated error mapping or historical evidence.


## Validation

Run the checker fixtures and its negative control, relevant existing failure-path tests, and `make verify`. If storage, native or build inputs change, also run the appropriate storage runner with a new evidence directory. Review diagnostics, the source paths actually scanned, and any intentionally unsupported syntax; do not claim semantic detection beyond the documented patterns.
