FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN apt-get update && apt-get install -y --no-install-recommends git curl ca-certificates gcc make perl xz-utils zlib1g-dev python3 && rm -rf /var/lib/apt/lists/*
COPY tests/git/install-oracle.sh /tmp/install-git-oracle.sh
RUN sh /tmp/install-git-oracle.sh && rm /tmp/install-git-oracle.sh
ENV PATH="/opt/git-oracle/bin:${PATH}"
# Preserve debug assertions while bounding acceptance-build disk usage.
ENV CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUN rustup component add rustfmt clippy
WORKDIR /repo
COPY Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml build.rs ./
COPY Makefile ./
COPY .dockerignore ./
COPY docs/zkit/letopis/mechanics/storage-schema.sql ./docs/zkit/letopis/mechanics/
COPY vendor ./vendor
COPY src ./src
COPY examples ./examples
COPY scripts ./scripts
COPY tests/*.rs ./tests/
COPY tests/support ./tests/support
COPY tests/fixtures/markdown ./tests/fixtures/markdown
COPY tests/fixtures/git ./tests/fixtures/git
COPY tests/fixtures/config ./tests/fixtures/config
COPY tests/fixtures/storage ./tests/fixtures/storage
COPY tests/storage ./tests/storage
ARG SOURCE_COMMIT=unknown
RUN echo "$SOURCE_COMMIT" > SOURCE_COMMIT && cargo fetch --locked
CMD ["python3", "-B", "tests/storage/run.py", "--platform", "host", "--evidence", "/receipt"]
