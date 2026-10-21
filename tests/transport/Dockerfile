FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
RUN rustup component add rustfmt clippy
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates openssl && rm -rf /var/lib/apt/lists/*
WORKDIR /repo
ENV TK6_DISPOSABLE_TRUST_STORE=1
