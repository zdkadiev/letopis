# Product storage acceptance

Use Python 3.11+ (macOS: `/opt/homebrew/bin/python3`):

```sh
/opt/homebrew/bin/python3 -B -m unittest discover -s tests/storage -p test_runner.py -v
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence NEW_DIRECTORY
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence OTHER_NEW_DIRECTORY
/opt/homebrew/bin/python3 -B tests/storage/negative_controls.py --evidence CONTROL_NEW_DIRECTORY
```

Evidence paths must not exist. Commands, exits, versions and SHA-256 input hashes
are retained with a fail-closed manifest. Host dependencies must already be fetched.
Linux image preparation may use network; runtime uses `--network none`, builds
from copied sources, and never mounts host target artifacts. Image inspection
records its architecture and identity. Container cleanup is separate from Docker
client process-group cleanup. Logs and receipts contain synthetic fixture data.

The copied executable accepts exactly one nonexistent scratch directory and checks
FTS, revision, native vector bytes/shared IDs, and persistence through public APIs.
Dynamic-link inspection rejects vec1/SQLite shared dependencies. Native unit tests
assert capabilities and read-back PRAGMAs before printing their provenance.
Negative controls archive committed sources, never modify the worktree, and use
fresh targets. Registration mutation must compile before its runtime rejection.

This does not exercise physical power loss, x86_64, resource/quality gates, complete
publication/CLI/ranking/provider/drain orchestration, or historical proof crates.
