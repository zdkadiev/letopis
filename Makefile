# Verify the product crate and its root tests with the pinned toolchain.
.PHONY: check verify mutants fmt

# Silent on success, full output on failure: keeps agent logs small. V=1 shows everything.
ifdef V
run = $(1)
else
run = @out=$$($(1) 2>&1) || { printf '%s\n' "$$out"; exit 1; }
endif

check:
	$(call run,cargo fmt --all --check)
	$(call run,cargo check --locked --offline --all-targets)
	$(call run,cargo clippy --locked --offline --all-targets -- -D warnings)
	$(call run,cargo test --locked --offline)
	@echo "check: ok"

# check plus strict Rustdoc and the release-profile test run.
verify: check
	$(call run,RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps)
	$(call run,cargo test --locked --offline --release)
	@echo "verify: ok"

# Mutation testing; extra cargo-mutants arguments via ARGS, e.g. ARGS='--in-diff mutants.diff'.
mutants:
	cargo mutants --cargo-arg=--locked --cargo-arg=--offline $(ARGS)

fmt:
	cargo fmt --all
