FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /checks
CMD ["python3", "run.py", "--system-sqlite"]
