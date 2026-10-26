# Rust interface mocks

Issue: https://github.com/zaurkadiev/letopis/issues/50

## Intent and scope

Use a Rust mocking library to create mocks of application-owned service interfaces, reduce repetitive one-off test doubles, and verify relevant service interactions. The change concerns test infrastructure and service tests; it does not change product behavior or the hexagonal architecture.

## Approaches

- Generate mocks from existing traits with Mockall: the user chose this approach because generated signatures follow the interfaces and avoid handwritten duplication.
- Define Mockall mocks manually in tests: avoids annotations on trait declarations but duplicates signatures and can drift.
- Retain only handwritten fakes: introduces no dependency but does not fulfill the requested library-based mocks.

## Design

Add a pinned, stable Mockall dev-dependency compatible with the pinned Rust toolchain and the locked/offline workflow. Use conditional `cfg(test)` Mockall generation on suitable application-owned interfaces. Generated mocks exist in unit-test builds only; production builds and public interfaces do not depend on Mockall. Keep reusable generated mocks reachable from the appropriate test modules without exporting test infrastructure in product builds.

Inventory the complete Rust test suite and apply generated mocks wherever they make existing interface-based tests clearer, covering meaningful success and failure interactions, including call counts and arguments where they are part of the behavior. Record the scope and why remaining hand-written doubles are retained. Retain stateful fakes and actual SQLite, process, and HTTP fixtures where they model behavior better. In particular, do not alter storage traits, generic associated lifetimes, lock semantics, or service architecture merely to accommodate Mockall. Record which interface shapes are unsuitable if the library cannot generate them without such changes.

Document how to use generated mocks and when a fake or real integration fixture is preferable. Do not add speculative production abstractions or rewrite all tests mechanically.

## Verification

Run focused tests with meaningful assertions, manually mutate a covered interaction to establish that the test detects it, restore the code, and rerun the test. Verify the dependency lock and offline workflow; run `make verify` on the actual final tree. Review architecture, production/test dependency separation, and the resulting test coverage. Report any unsupported interface or environment limitation.
