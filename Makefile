CARGO ?= cargo
V ?= 0
MAKEFLAGS += --no-print-directory

.PHONY: check verify fmt release _check

# Capture one step at a time so a failure retains its complete diagnostic output.
define run
	@if [ "$(V)" = "1" ]; then $(1); else \
		log=$$(mktemp) || exit $$?; \
		if $(1) >"$$log" 2>&1; then status=0; else status=$$?; cat "$$log"; fi; \
		rm -f "$$log"; exit $$status; \
	fi
endef

check: _check
	@printf 'ok\n'

verify: _check
	$(call run,RUSTDOCFLAGS='-D warnings' $(CARGO) doc --locked --offline --no-deps)
	$(call run,$(CARGO) test --locked --offline --release)
	@printf 'ok\n'

_check:
	$(call run,$(CARGO) fmt --all --check)
	$(call run,$(CARGO) check --locked --offline --all-targets)
	$(call run,$(CARGO) clippy --locked --offline --all-targets -- -D warnings)
	$(call run,$(CARGO) test --locked --offline)

release:
	$(call run,$(CARGO) build --locked --offline --profile dist)
	@ls -lh target/dist/letopis | awk '{print $$5, $$9}'

fmt:
	$(CARGO) fmt --all
