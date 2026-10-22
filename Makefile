CARGO ?= cargo

.PHONY: check verify mutants fmt

check:
	$(CARGO) fmt --all --check
	$(CARGO) check --locked --offline --all-targets
	$(CARGO) clippy --locked --offline --all-targets -- -D warnings
	$(CARGO) test --locked --offline

verify: check
	RUSTDOCFLAGS='-D warnings' $(CARGO) doc --locked --offline --no-deps
	$(CARGO) test --locked --offline --release

mutants:
	$(CARGO) mutants --cargo-arg=--locked --cargo-arg=--offline $(ARGS)

fmt:
	$(CARGO) fmt --all
