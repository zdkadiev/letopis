FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN apt-get update && apt-get install -y --no-install-recommends git python3 && rm -rf /var/lib/apt/lists/*
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
COPY tests/storage ./tests/storage
ARG SOURCE_COMMIT=unknown
RUN echo "$SOURCE_COMMIT" > SOURCE_COMMIT && cargo fetch --locked
CMD ["python3", "-B", "tests/storage/run.py", "--platform", "host", "--evidence", "/receipt"]
