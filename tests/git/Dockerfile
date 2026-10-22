FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy
WORKDIR /repo
COPY Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml ./
COPY build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY tests/*.rs ./tests/
COPY tests/support ./tests/support
COPY tests/fixtures/markdown ./tests/fixtures/markdown
RUN cargo fetch --locked
CMD ["sh", "-c", "git --version && rustc --version && cargo --version && uname -sm && cargo test --locked --offline && cargo fmt --all --check && cargo clippy --locked --offline --all-targets -- -D warnings"]
