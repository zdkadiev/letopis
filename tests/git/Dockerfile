FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN apt-get update && apt-get install -y --no-install-recommends git curl ca-certificates gcc make perl xz-utils zlib1g-dev strace && rm -rf /var/lib/apt/lists/*
COPY tests/git/install-oracle.sh /tmp/install-git-oracle.sh
RUN sh /tmp/install-git-oracle.sh && rm /tmp/install-git-oracle.sh
ENV PATH="/opt/git-oracle/bin:${PATH}"
RUN rustup component add rustfmt clippy
WORKDIR /repo
COPY Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml ./
COPY build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY tests/*.rs ./tests/
COPY tests/support ./tests/support
COPY tests/fixtures/markdown ./tests/fixtures/markdown
COPY tests/fixtures/git ./tests/fixtures/git
COPY tests/fixtures/config ./tests/fixtures/config
COPY tests/fixtures/storage ./tests/fixtures/storage
RUN cargo fetch --locked
CMD ["sh", "-c", "git --version && rustc --version && cargo --version && uname -sm && cargo test --locked --offline && cargo fmt --all --check && cargo clippy --locked --offline --all-targets -- -D warnings"]
