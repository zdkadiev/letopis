# TK-11 CLI runtime receipts

The runner verifies the product CLI transport and real filesystem/process adapters.
Successful check/inspect envelopes come only from explicitly injected handler unit
tests. The product returns `internal_error` / `operation unavailable in this build`
for valid check, inspect and internal drain requests until their services exist.
Real drain acceptance remains open for Task 5 with TK-13; inspect/check semantics
belong to TK-12/TK-19. These receipts do not establish operational idempotency.

Use Python 3.11+ with the development dependency `jsonschema==4.26.0` prepared in
an isolated environment; it is not a product dependency. For this worktree:

```sh
.artifacts/tk11/python-env/bin/python -B -m unittest discover -s tests/cli -p test_runner.py -v
.artifacts/tk11/python-env/bin/python -B tests/cli/run.py --platform host --evidence .artifacts/tk11/host-NEW
.artifacts/tk11/python-env/bin/python -B tests/cli/run.py --platform linux --evidence .artifacts/tk11/linux-NEW
.artifacts/tk11/python-env/bin/python -B tests/cli/negative_controls.py --evidence .artifacts/tk11/controls-NEW
./scripts/check-rust.sh
RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps
```

Each evidence path must be new. Manifests retain exact input hashes, source commit
and dirty status, artifact hashes and copies, argv, cwd, input bytes/hash, exit
codes, stdout/stderr, versions and selected build environment variables. The X-1
JSON-compatible YAML catalogue is parsed by `json.loads`; Draft202012Validator
selects the command's success/error definition rather than the broad catalogue
union. Handler responses are labelled separately from actual product outputs.
`runtime.log` and `observers.json` retain the real socket observations for Task 3
schedules; `locks.log` runs the Task 2 filesystem tests, including native flock
exclusion and hardlink rejection. The crate's two ignored child entry helpers are
invoked by their parent tests, not accepted as skipped lock coverage.

Linux is built from current working source with prepared locked Cargo dependencies
and a pinned jsonschema installation. Runtime uses `--network none`, with no
package fetch. The runner compares every inventoried source hash with its container
receipt. Compare host `manifest.json.inputs` with Linux `manifest.json.inputs`
and `container/manifest.json.inputs` for the same checkpoint. Source changes during
a run fail acceptance. Inventory includes test support, fixtures, schema and Docker
inputs; historical harnesses and generated build trees are not part of this suite.

The runner terminates/reaps its own command session on timeout; Docker uses a
unique owned container name and cleans that container even after a client failure.
Fixture peers are bounded by their owned socket and existing RAII cleanup. No PID
read from lock files is signalled. Caller-supplied fixture overrides are removed;
the tests receive the fixture just built from the measured source.

Negative controls copy the current source into a disposable directory. They remove
null stdout/stderr redirection and kernel exclusion independently. Each altered
source must compile and fail the named behavioral observer; source bytes are then
restored exactly and that observer must pass. The original worktree is never mutated.
No claim is made about historical proof harnesses, real providers, handled SIGINT,
Windows/NFS, power loss, or untested architectures.
