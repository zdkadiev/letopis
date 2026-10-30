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
format and lint checks offline. The build copies `clippy.toml` so Linux enforces
the same 60-code-line function limit as the host. It does not mount host binaries. The Rust base
image is digest-pinned; the actual fixture Git version and resulting image ID
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

Historical TK-8 receipts assessed the CLI adapter at their captured revisions.
The current product uses gix in process and has no runtime Git version gate.
Git in this image prepares fixtures and independent oracles, including SHA-256. Physical
power-loss durability, x86_64, resource limits and atomic live filesystem capture
remain outside TK-8. Recorded evidence lives in `docs/zkit/letopis/checks/TK-8/`.


The adapter requires a self-contained object store. It rejects any existing
`objects/info/alternates` path, including empty/inactive files and symlinks,
at open and before every create/read/pin. Git resolves the path through its
actual gitdir/common-dir; linked layout is covered without adding multi-worktree
product semantics. Shared/reference clones need a self-contained clone; inactive
alternates configuration must be removed separately. The adapter neither reads
donor paths into diagnostics nor materializes/repacks objects. Regressions cover
an ordinary shared clone, actual donor GC, late configuration and inactive
common-dir alternates in both formats. Concurrent object-store configuration
changes during an operation remain outside the accepted same-UID-writer scope.
Checkout roots with non-UTF-8 names remain safely unsupported; no byte-path API
is provided.


## Product helper denial — issue #67, 2026-10-31

`git_no_subprocess` prepares fixtures with real Git before invoking the absolute
product executable. Its PATH contains only a Git poison executable which records
attempts and fails. The matrix covers init/config, check/status, FTS search,
inspect/traverse, refresh/reindex, disabled drain, corrupt/missing objects and IDs,
malformed config, alternates and outside/bare roots. A separate nonempty direct
drain uses an owned loopback fake provider, never a real model.

PATH denial alone cannot detect absolute executables. On Linux additionally run:

```sh
docker build -f tests/git/Dockerfile -t letopis-issue67-git .
docker run --rm --network none \
  -e LETOPIS_TEST_STRACE=/usr/bin/strace \
  -e LETOPIS_TEST_EVIDENCE=/receipt \
  letopis-issue67-git \
  cargo test --locked --offline --test git_no_subprocess -- --nocapture
```

`strace -ff -qq -e trace=process` traces each product invocation and all descendants;
fixtures/oracles and the fake server execute outside that interval. The test
requires exactly one execve of the absolute Letopis binary in these intervals;
this matrix deliberately gives foreground commands disabled embeddings, and tests
nonempty work through direct drain. Existing CLI drain tests separately preserve
eligible detached Letopis execution. Threads are allowed. Trace files and argv
are retained beneath `LETOPIS_TEST_EVIDENCE/git-traces` when configured; copy
`/receipt` from an owned named container before removing it to retain evidence.

The native Linux runner can supply the complete unchanged root/debug/release/lint/
strict-doc suites; then use this image only for the additional focused trace gate.
Record both exact image IDs, input hashes and commands rather than treating a
prior revision's result as current. `.dockerignore` explicitly admits the collision
fixture needed by `include_bytes!` and both Dockerfiles copy its directory.
No power-loss, arbitrary filesystem, all-kernel-interleaving or x86_64 claim follows.


### Strict oracle toolchain

The approved malformed-tree reference is Git 2.50.1, matching the frozen host
baseline, rather than the acceptance of every historical Git version. Debian
bookworm's Git 2.39.5 performs parsing-only checks for `hash-object -t tree` and
accepts several trees the strict reference rejects. Both current Docker images
run the small [installer](install-oracle.sh): upstream 2.50.1 source archive,
SHA-256 verified before extraction/build, installed under `/opt/git-oracle` and
selected first in PATH. Its `SOURCE` file records URL, hash, tag and commit.
Build prerequisites are declared in the Dockerfiles; this tool is not linked
into or required by Letopis. The network-disabled test run does not install tools.

Archive SHA-256: `7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4`.
Tag `v2.50.1`, commit `d82adb61ba2fd11d8f2587fca1b6bd7925ce4044`.
Host differential tests also need the strict 2.50.1 oracle behavior; a failed
comparison with an older Git is not permission to weaken the validator or skip
the case. Cross-version malformed-tree acceptance parity is not claimed.
