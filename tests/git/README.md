# Git snapshot verification

From the repository root:

```sh
cargo +1.95.0 test --locked --offline
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings
docker build -f tests/git/Dockerfile -t letopis-tk8-check .
docker run --rm --network none letopis-tk8-check
```

Build installs Git, rustfmt and Clippy and fetches locked Rust dependencies.
Runtime recompiles the copied source and executes the complete root suite,
format and lint checks offline. It does not mount host binaries. The Rust base
image is digest-pinned; the actual Debian Git version and resulting image ID
are recorded with each verification receipt.

The public snapshot and retention tests run against both SHA-1 and SHA-256
repositories, with isolated fixture configuration and staged/unstaged files.
Only disposable test repositories undergo reflog expiry, GC/prune or corruption.
Owned child supervisors bound the I/O, lazy-fetch and concurrent-pin fixtures;
there is no production timeout. The transport test first proves its disposable
remote/helper is reachable, then checks that snapshot operations never invoke it.

A separate hostile-routing run can use temporary trap locations:

```sh
GIT_DIR=/tmp/letopis-trap/git GIT_WORK_TREE=/tmp/letopis-trap/work \
GIT_INDEX_FILE=/tmp/letopis-trap/index GIT_OBJECT_DIRECTORY=/tmp/letopis-trap/objects \
cargo +1.95.0 test --locked --offline --test git_snapshots --test git_retention
```

Minimum Git 2.29 is covered by version-gate tests and the documented command
subset; these commands do not claim an actual Git 2.29 execution. Physical
power-loss durability, x86_64, resource limits and atomic live filesystem capture
remain outside TK-8. Recorded evidence lives in `docs/zkit/letopis/checks/TK-8/`.
