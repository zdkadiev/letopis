# Product storage acceptance

Use Python 3.11+ (macOS: `/opt/homebrew/bin/python3`):

```sh
/opt/homebrew/bin/python3 -B -m unittest discover -s tests/storage -p test_runner.py -v
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk-9/host-NEW
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk-9/linux-NEW
/opt/homebrew/bin/python3 -B tests/storage/negative_controls.py --evidence .artifacts/tk-9/controls-NEW
```

Keep raw receipts under ignored `.artifacts/` locally or in CI artifacts.
Commit only concise final reports and source-hash summaries under `docs/`.
Input inventories prune `target/`, `.build/`, and `__pycache__/` directories.
Evidence paths must not exist. Commands, exits, versions and SHA-256 input hashes
are retained with a fail-closed manifest. Host dependencies must already be fetched.
Linux image preparation may use network; runtime uses `--network none`, builds
from copied sources, and never mounts host target artifacts. Image inspection
records its architecture and identity. Container cleanup is separate from Docker
client process-group cleanup. Logs and receipts contain synthetic fixture data.

The copied executable accepts exactly one nonexistent scratch directory and checks
FTS, revision, native vector bytes/shared IDs, and persistence through public APIs.
Dynamic-link inspection rejects vec1/SQLite shared dependencies. Native unit tests
assert SQLite >=3.51.3, the vec1 version 0.8 prefix, FTS capabilities and
read-back PRAGMAs before printing their provenance. SQLite 3.53.2 and the exact
vec1 NEON/multi-threaded description are observed values, not exact assertions.
The host runner also executes the focused Python suite and canonical-DDL parity
check. Parity compares the fixed cache/vector fixtures after removing comments
and the intentional project seed; its regression test deliberately changes a
column type and requires rejection. This is not a general SQL parser.
Negative controls archive committed sources, never modify the worktree, and use
fresh targets. Registration mutation must compile before its runtime rejection.

This does not exercise physical power loss, x86_64, resource/quality gates, complete
publication/CLI/ranking/provider/drain orchestration, or historical proof crates.
