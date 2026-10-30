FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN apt-get update && apt-get install -y --no-install-recommends git python3 python3-venv && rm -rf /var/lib/apt/lists/*
RUN python3 -m venv /opt/cli-python && /opt/cli-python/bin/pip install --no-cache-dir jsonschema==4.26.0
RUN rustup component add rustfmt clippy
WORKDIR /repo
COPY Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml build.rs .dockerignore ./
COPY Makefile ./
COPY vendor ./vendor
COPY src ./src
COPY examples ./examples
COPY scripts ./scripts
COPY tests/*.rs ./tests/
COPY tests/support ./tests/support
COPY tests/fixtures/markdown ./tests/fixtures/markdown
COPY tests/fixtures/git ./tests/fixtures/git
COPY tests/cli ./tests/cli
COPY skills/letopis/SKILL.md ./skills/letopis/SKILL.md
COPY docs/zkit/letopis/contracts/schemas/X-1-cli.yaml ./docs/zkit/letopis/contracts/schemas/
ARG SOURCE_COMMIT=unknown
RUN echo "$SOURCE_COMMIT" > SOURCE_COMMIT && cargo fetch --locked
CMD ["/opt/cli-python/bin/python", "-B", "tests/cli/run.py", "--platform", "host", "--evidence", "/receipt"]
