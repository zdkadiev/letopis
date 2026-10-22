#!/bin/sh
# Verify the product crate and its root tests with the pinned toolchain.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"

cargo fmt --all --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
