---
name: google-rust-style
description: Apply Google's Rust style guide to Letopis. Use whenever writing, reviewing, or refactoring Rust code in this repository, or when the user mentions Google Rust style, ownership and borrowing, Rc/Arc, clone, error handling with Option/Result/panic, newtypes, flexible reference arguments, iterator returns, trait or API design, naming, documentation, unsafe code, or style review. Also use before declaring a Rust change complete, to check the diff against the project rules and the Google guide.
---

# Google Rust style for Letopis

Use the Google Rust Style Guide as the default for questions the Letopis documents do
not answer. It is a style and API-design guide, not a reason to override the project.

## Precedence

Read the relevant sources before applying style rules:

1. `AGENTS.md`, the task card under `docs/zkit/letopis/roadmap/epics/*/tasks/`, and any
   agreed spec or plan under `docs/superpowers/`.
2. `docs/zkit/letopis/architecture.md`, `decisions.md`, `mechanics.md`, and the relevant
   contracts.
3. `docs/development.md` for the pinned toolchain, checks, and project lint policy.
4. This skill for choices those sources leave open.

If a Google rule conflicts with a Letopis rule, follow Letopis, keep the deviation
narrow, and say why in the change summary or review comment.

Letopis is application code. Google's API-design rules are strong recommendations here,
not a licence to add reusability layers. Prefer the simplest direct solution that meets
the current requirement. Apply the API rules strictly only to genuinely reusable public
library surfaces. Rules marked `[chrome]` or `[Fuchsia]` do not apply to this project.

## Workflow

1. Read the affected code, its callers, and the task contract before changing style.
2. Make the smallest coherent change. Do not reorder or restyle untouched code.
3. Apply the rules below while writing, not as a separate cleanup pass.
4. Update a comment in the same change that alters the behaviour it describes.
5. Run `make fmt`, then `make check`; run `make verify` when `src/`, public documentation,
   or dependencies change. Add platform or storage runners when the task requires them.
6. In review, name the rule and explain the concrete risk it prevents. Do not cite style
   as a substitute for understanding the contract.

## Follow public Rust rules

Follow the official Rust Style Guide and Rust API Guidelines where they do not conflict
with Google's guide or with Letopis. Rustfmt owns formatting. Letopis keeps stock rustfmt
settings, including the 100-column limit. Use `make fmt`; do not hand-format around it.

- Use `#[rustfmt::skip]` only for manual alignment or generated code. Never for personal
  preference.

## Code organization

- Do not use glob imports. Allowed exceptions are `use super::*` in inline modules,
  `use enum::*` in enum-focused bodies, `pub use submodule::*` in non-leaf modules, and
  private C++ namespace merging.
- One `use` statement imports from one module. Do not write `use foo::{bar::Bar, Baz}`;
  write separate statements.
- Import traits inside the function that uses them unless the trait is used throughout
  the file.
- Qualify calls to top-level functions from other modules: `mem::transmute(y)`, not
  `transmute(y)`. Exceptions are disambiguated C++ functions, small single-purpose
  first-party crates, and collision-prone module names.
- Import type names and use them unqualified: `Regex`, not `regex::Regex`. In ambiguous
  cases use `io::Error` or an alias such as `IoError`.
- Non-leaf modules contain module declarations and re-exports, not substantial API
  implementations. This is subordinate to the Letopis layout: `service/mod.rs` declares
  interfaces and re-exports, while concrete implementations live in their own files.
- Name a leaf module after its primary item: `Foo` lives in `foo.rs` / `mod foo`.
- Small crates and flat file layout are Chrome build rules, not Letopis rules. Keep the
  Cargo and module layout required by `AGENTS.md` and the architecture.

## Ownership and borrowing

### Avoid `Rc` and `Arc`

Prefer unique ownership and borrowing. Use `Rc` or `Arc` only when the alternative
ownership structure would be unreasonably complex to write, understand, or maintain.
That is a code-review judgement, not a default. Shared-ownership pointers add runtime
overhead and their own complexity, and are easy to reach for too quickly.

### Prefer borrows over deep copies

Call `.clone()` when a deep copy is genuinely needed, not as a substitute for designing
ownership. If the borrowing relationships become excessively complex, cloning can be a
pragmatic exception. Consider performance. Cloning `Rc` or `Arc` is fine: it copies the
pointer, not the data.

Also avoid taking ownership when a borrow is enough; the project enables
`needless_pass_by_value`.

## Function inputs and outputs

### Accept flexible references for immutable data

Pass a reference to the data itself, not to a smart pointer, container, or wrapper.

| Use | Instead of | Why |
| --- | --- | --- |
| `&T` | `&Box<T>` | Borrow the data inside the box |
| `&[T]` | `&Vec<T>` | Accept any slice, not one container |
| `&str` | `&String` | Accept a string slice, not an owned-string reference |
| `Option<&T>` | `&Option<T>` | Borrow the data inside the `Option` |

Exception: non-linear character data such as `Rope` or `Cord`. Clippy's `borrowed_box`
and `ptr_arg` cover common cases. This makes functions more versatile and lets callers
pass data without copying it first.

### Iterators versus owned collections in return types

Prefer returning an iterator, either a concrete type or `impl Iterator`, over `Vec<T>`.
In application code, returning `Vec<T>` is fine when it simplifies the code and the
flexibility or performance advantage is not clearly needed.

Returning a vector is also reasonable when most callers need one, the implementation
naturally constructs one, random indexing is likely, a `for` loop is clearer and the
allocation does not matter, the function's purpose is to create a collection, or it
returns a pre-existing vector.

### Iterators versus slices in parameters

Reusable library code that only iterates over its input should accept an iterator or
`impl IntoIterator`. In application code, a slice `&[T]` is fine when it simplifies the
API and the flexibility or performance advantage is not clearly needed.

### Accepting `Into`

Consider `impl Into<T>` only for reusable library APIs where call-site ergonomics are
paramount, and weigh the generic code-size and inference costs. Application code should
almost never use `impl Into<T>` parameters.

### Prefer `enum` over `bool` in APIs

Use an enum parameter when a `bool` argument's purpose is not obvious. If a `bool`
remains, document its purpose with an inline comment.

### Reporting errors

Choose the mechanism that fits the contract:

- `Option` for simple, anticipated failures whose cause is obvious, such as a missing
  value, a parse mismatch, or integer overflow.
- `Result` for recoverable failures where the caller needs context. External factors
  such as I/O and network errors are typical. The error must let the caller distinguish
  failure modes and recover appropriately.
- `panic!` for bugs and violated invariants: a missing check or an unmet precondition.
  Fix the defect in code, not at runtime.

Tests may use panics for assertions. Letopis production code must return typed `Result`
for expected failures. A panic is reserved for a detected programming bug. Do not use
`catch_unwind` as ordinary error handling; see the implementation rules below.

## Types

### Newtypes versus type aliases

Use a newtype instead of a type alias when:

- the type has notable semantic differences;
- it warrants an API of its own;
- the underlying type may change later, especially if you do not control all usages;
- type safety matters because values are easy to mix up. A `Date` type may use newtypes
  to distinguish `Day`, `Month`, and `Year` values in its calculations.

### Prefer named fields over tuples

Prefer named fields over tuples and non-newtype tuple structs; `.0` and `.1` quickly
become confusing. Newtype tuple structs with one field are fine, as are very local
contexts where confusion is unlikely.

## Traits

- Add trait implementations when they become necessary in application code. Low-level
  library code may anticipate users' needs.
- If a method matches a standard-library trait, implement the trait. For example,
  implement `FromStr` instead of a custom `from_str` method. Keeping the method too is
  acceptable for discoverability.
- Use `#[derive()]` when it can express the implementation.
- In libraries, prefer generic parameters (`T: Trait` or `impl Trait`) over `dyn Trait`
  unless code size matters. Generics avoid fat pointers and vtable dispatch and permit
  inlining. Some traits are not dyn-compatible.
- Define a trait only to enable generic programming, to accept or return `impl Trait` or
  `dyn Trait`, or to define an extension trait. Otherwise use an inherent implementation.
  Do not use traits merely to organize code.
- Name extension traits with an `~Ext` suffix, such as `ForeignTypeExt`. The team that
  owns the type should own its extension trait. Extension traits are not for generics or
  dynamic dispatch.
- Important traits:
  - Error types should implement `Copy` whenever possible, unless they contain
    non-`Copy` fields.
  - Most types should implement `Clone` unless they represent an external resource.
  - Implement `Default` when the type has `new()` or a natural default state.
  - Prefer `PartialEq` over `Eq` for `==`; `Eq` adds reflexivity requirements.
  - Prefer `PartialOrd` over `Ord` for comparisons; `Ord` requires total ordering.
  - Implement `Hash` and `Eq` when a type is likely to become a map key or cache key; add
    `Ord` when it has a clear total ordering.
- If an important `From` impl is a primary constructor, add an inherent `new()`,
  `new_from_foo()`, or `from_foo()` method.
- A struct with all-public fields used as a bag of options should implement `Default`,
  enabling functional record update syntax.

## Concurrency

- Never reach inside a poisoned mutex. Do not use poison as a recovery mechanism.
- Do not add `async`/`.await` `[chrome]` or start threads through the Rust standard
  library `[chrome]`. Letopis is synchronous by design; do not add an async runtime
  without an agreed requirement.
- Fuchsia's mutex rule does not apply.

## Macros

Prefer, in order:

1. ordinary functions, types, traits, objects, and modules;
2. `macro_rules!` macros;
3. other code generation;
4. procedural macros.

Avoid macros and metaprogramming unless they pull their weight. They weaken IDE and code
search support, create language dialects, and make large-scale changes harder.

## Naming

- Follow standard-library method naming. Use `as_...`, `to_...`, and `into_...` with the
  correct `self` convention.
- Give lifetimes descriptive names, especially type lifetimes. `'_` is fine where it
  works. Single-character lifetimes are acceptable for a simple input/output
  relationship or a local higher-ranked bound; use descriptive names beyond two.
- Use short generic parameter names related to the kind of value, often suggested by the
  main trait bound. `T` and `U` are fine in largely unbound APIs such as `Vec<T>`.
- Name extension traits with an `~Ext` suffix.
- Constructors: `new()`, or `new_with_foo()` when there is more than one. A fallible
  constructor returning `Result` is `try_new()`.
- Value-to-value conversions: `from_foo()` together with a `From` impl.
- Avoid abbreviations, especially acronyms and initialisms unknown outside the project.
  Common Wikipedia abbreviations and clear names such as `IpAddr` are exceptions.

## Documentation

- Document public APIs. Consider enabling `missing_docs` where appropriate.
- Include examples somewhere enough to show how the crate works; not every function
  needs one.
- Always document panic cases for functions that may panic on malformed input.
- Clearly document invariants for unsafe functions and traits.
- Use capitalization and punctuation.
- Do not explain the programming language or repeat the function name. Omit a comment
  that says nothing beyond the code.
- Use `///` for the item that follows and `//!` for the module or crate, at the top of the
  file. Use `//` for implementation comments and TODOs not meant for users.
- Use Markdown in doc comments and follow the official Rust doc-comment rules.
- Prefer straightforward doc comments over macro-generated documentation.
- Letopis requires `# Errors` on public fallible APIs and `# Panics` when a panic is
  possible. Do not add empty sections.

## Other Rust features

- Do not write `unsafe` unless safe code cannot implement the same functionality.
  Encapsulate it behind a safe abstraction, keep it away from business logic, and
  preferably isolate it in a separate crate with few dependencies.
- Every unsafe block needs a `SAFETY:` comment explaining why it is correct, plus a
  technical justification: why safe code cannot do it, and benchmarks if the reason is
  performance.
- Do not use unstable features.
- Getters and setters:
  - `foo()` returns `&T`, or `T` by value when `T: Copy`.
  - `foo_mut()` returns `&mut T`.
  - `take_foo()` clears the value and returns the old one.
  - `set_foo()` takes `T` by value.
  - `with_foo()` / `with_mut_foo()` access through a closure.
  - `replace_foo()` sets the value and returns the old one.
- Implement expected API sets together: if there is `len()`, add `is_empty()`.
- Treat items marked `#[doc(hidden)]` as private unless you own them.
- Mark a public function `const` only if you expect it to remain `const` or will help
  clients migrate. Removing `const` is a breaking change.
- Do not overload operators for non-mathematical convenience.
- Mark publicly exposed enums `#[non_exhaustive]` when other teams use them and they may
  change. Ontology enums and two-state enums that could be booleans are exceptions, as is
  a committed large-scale change to fix breakages.
- Mark public structs with public fields `#[non_exhaustive]` when other teams use them
  and fields may change. Do not do this when functional record update is required.
- Use `#[must_use]` on a function whose ignored return value makes correct use
  impossible, or whose return value is an error or other critical information. If the
  return type already carries `#[must_use]`, the function does not need it.
- Use `#[must_use]` on types intended for such returns, notably error types and lazily
  evaluated computations.
- Do not run code before `main()`, including through crates such as `ctor`.

## Implementation concerns

- Do not overuse type inference. Add enough explicit types for a familiar reader to
  follow the code. Prefer annotating variable declarations over turbofish; a `_`
  inference marker is acceptable when the compiler requires a turbofish. Annotate
  generic calls such as `.into()` and `.parse()` and the results of complex iterator
  chains.
- Prefer pattern matching over check-then-unwrap. Do not use a potentially panicking
  operation just because the input is known to satisfy it; prove correctness to the
  compiler:

```rust
// Good: the pattern proves the value is `Some`.
if let Some(value) = option {
    use_value(value);
}

// Bad: correctness relies on a separate runtime check.
if option.is_some() {
    use_value(option.unwrap());
}
```

- If a panic is necessary because an invariant cannot be expressed in the type system,
  Google prefers `expect` over `unwrap` to document the reason. Letopis is stricter:
  production code must not use `unwrap` or `expect` for expected failures. `expect` is
  allowed only for an invariant guaranteed by construction, with a narrow
  `#[expect(clippy::expect_used, reason = "...")]`; return a typed `Result` otherwise.
- Do not omit patterns on a `non_exhaustive` enum. List all variants you know and put
  ignored variants in the wildcard arm. If you only care about specific variants, use
  `if let` or `let ... else`.
- Add context to assertions when it helps explain why the assertion should hold, what
  value failed, or whether the caller or implementation is at fault. Use the format
  string rather than a nearby comment. Do not merely repeat the condition.
- Avoid long or complex iterator combinator chains. Do not build a chain in a `for` loop
  header; assign it to a named variable first. Avoid `Iterator::for_each` unless there is
  a good reason; a `for` loop is usually clearer. Short chains and combinators that carry
  real logic, such as `any()`, are preferable.
- Do not call `panic::catch_unwind` in first-party code. A defence-in-depth wrapper at a
  critical entry point is acceptable only if the codebase does not rely on it, the code
  is built with `panic=unwind`, and the reviewer accepts the exception. Ordinary
  recovery must use `Result`.
- Prefer `format!` and `write!` over `+` for concatenating more than two strings, and
  over `str::replace` for substitutions in a literal string. They allocate less and
  avoid inventing a template syntax.

## Testing

Use the project's test framework: `cargo test` through `make check` / `make verify`, with
Given / When / Then comments required by `AGENTS.md`. Google's per-codebase test-framework
rule means the project's choice wins; do not introduce a second test framework.

Tests may use `unwrap` and panics for assertions. Assertions should include context when
the failure would otherwise be unclear. Test names describe the behaviour, not the
implementation.

## Conflict resolution

| Google guidance | Letopis rule | Apply |
| --- | --- | --- |
| Prefer `expect` over `unwrap` | No `unwrap`/`expect` in production | Typed `Result`; `expect` only for a construction invariant with `#[expect]` |
| `panic!` for bugs | Same intent, but expected failures use typed errors | Panic only for a detected programming bug; tests may assert |
| `catch_unwind` is prohibited except defence in depth | No recovery by unwinding | Return `Result`; do not rely on `catch_unwind` |
| Non-leaf modules hold declarations and re-exports | `service/mod.rs` declares interfaces and re-exports; implementations live separately | Follow the Letopis module layout |
| Prefer generics over `dyn Trait` in libraries | Choose concrete types, traits at real boundaries; avoid wrapper layers | Follow Letopis; use generics only for a concrete need |
| Application code may use `Vec` returns and slice parameters | KISS/YAGNI, but API clarity still matters | Use the simpler application-code form when the advantage is not clear |
| Chrome/Fuchsia build, thread, async, and mutex rules | Letopis is a Cargo CLI, synchronous by design | Ignore those codebase-specific rules |

## Review checklist

- Ownership: no `Rc`/`Arc` or `clone()` without a reason; references are as flexible as
  the data allows.
- Errors: `Option`, `Result`, or panic matches the contract; no `unwrap`/`expect` in
  production; public `# Errors` / `# Panics` sections are accurate.
- Types: newtypes where semantics or safety demand; named fields over tuples; enums over
  ambiguous booleans.
- API: traits exist for a reason; standard traits and `derive` are used; naming follows
  the standard library; extension traits use `Ext`.
- Documentation: public contracts, invariants, panics, and examples are documented.
- Implementation: explicit enough types, pattern matching instead of check-then-unwrap,
  readable iteration, assertion context, no `catch_unwind`.
- Verification: `make fmt`, `make check`, and `make verify` when required; report any
  check not run and why.
- Deviation: any departure from Google's guide is intentional, project-justified, and
  called out in the change summary.

Source: [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html),
licensed CC BY 3.0. This skill condenses and adapts it for Letopis; consult the upstream
page for full rationale and examples.
