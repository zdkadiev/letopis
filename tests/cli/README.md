# CLI runtime, search and inspection receipts

The runner verifies the combined root crate, product CLI transport, real filesystem
and process adapters, and the actual embedding drain against controlled loopback
Ollama HTTP. `cli_drain` publishes through Documents and checks stored vectors,
durable attempts, config/text races, lost responses, process death and no-work
launch suppression. Its foreground fixture calls the production service/launcher;
the embedding executor is always the actual product executable.

Search and knowledge inspection use the actual product executable. `cli_reading`
asserts one JSON line, empty stderr and exact exit before emitting `PRODUCT_READING`
observations for search success, provider-failure FTS fallback, retained-tree inspect,
not-found and malformed-input errors. The runner validates each against its matching
X-1 command schema and records `product-reading-responses.json`. Injected
`CLI_RESPONSE` receipts remain separately labelled and cannot satisfy this coverage.
Full source inspection and check semantics remain TK-24/TK-19 work.

Use Python 3.11+ with the development dependency `jsonschema==4.26.0` prepared in
an isolated environment; it is not a product dependency. For this worktree:

```sh
.artifacts/tk11/python-env/bin/python -B -m unittest discover -s tests/cli -p test_runner.py -v
.artifacts/tk11/python-env/bin/python -B tests/cli/run.py --platform host --evidence .artifacts/tk12/host-NEW
.artifacts/tk11/python-env/bin/python -B tests/cli/run.py --platform linux --evidence .artifacts/tk12/linux-NEW
.artifacts/tk11/python-env/bin/python -B tests/cli/negative_controls.py --evidence .artifacts/tk11/controls-NEW
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
exclusion and hardlink rejection. `product-drain.log` and `product-observations.json`
retain actual product output, detached completion and foreground EOF observations.
Both platforms first run `make verify` (debug checks, strict Rustdoc and release
tests), including the merged document,
filesystem, storage and provider regressions. Three ignored child entry helpers
are explicitly driven by parent tests. The fourth ignore is TK-13's separate real
60-second provider deadline acceptance: it is deliberately not rerun here; its
original evidence remains pinned to TK-13. Nested filtered helper results are
reported separately from root-suite totals.

Linux is built from current working source with prepared locked Cargo dependencies
and a pinned jsonschema installation. Runtime uses `--network none`, with no
package fetch. The runner compares every inventoried source hash with its container
receipt. Compare host `manifest.json.inputs` with Linux `manifest.json.inputs`
and `container/manifest.json.inputs` for the same checkpoint. Source changes during
a run fail acceptance. Inventory includes test support, fixtures, schema and Docker
inputs; historical harnesses and generated build trees are not part of this suite.

The runner terminates/reaps its own command session on timeout; Docker uses a
unique owned container name and cleans that container even after a client failure.
Socket fixture peers use their existing RAII cleanup. The product fixture also
tracks only the drain it launched inside its isolated repository, excluding the
known foreground/test PID; on failed assertions it terminates that owned drain.
Product code never uses PID text for ownership or signal selection. Detached
completion accepts an exited Linux zombie when minimal container PID 1 has not
yet reaped it; direct product invocations additionally assert exit 0.
Caller-supplied fixture overrides are removed;
the tests receive the fixture just built from the measured source.

For TK-12 use `negative_controls.py --suite reading --evidence <new-directory>`
for captured configuration, candidate hash validation, cache reopening after inference,
and live inspection rereads. The default `runtime` suite retains the historical
null-output/kernel-exclusion controls; selecting reading does not rerun those.

Negative controls copy the current source into a disposable directory. They remove
null stdout/stderr redirection and kernel exclusion independently. Each altered
source must compile and fail the named behavioral observer; source bytes are then
restored exactly and that observer must pass. The original worktree is never mutated.
Fixture paths come from Cargo JSON artifacts, including when `CARGO_BUILD_TARGET`
selects a target-specific output directory.
No claim is made about historical proof harnesses, real providers, handled SIGINT,
Windows/NFS, power loss, or untested architectures.
