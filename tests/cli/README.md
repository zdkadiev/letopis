# CLI runtime, search and inspection receipts

The runner verifies the combined root crate, product CLI transport, real filesystem
and process adapters, and the actual embedding drain against controlled loopback
Ollama HTTP. `cli_drain` publishes through Documents and checks stored vectors,
durable attempts, config/text races, lost responses, process death and no-work
launch suppression. Its foreground fixture calls the production service/launcher;
the embedding executor is always the actual product executable.

Search and inspection use the actual product executable. `cli_reading`
asserts one JSON line, empty stderr and exact exit before emitting `PRODUCT_READING`
observations for search success, provider-failure FTS fallback, retained-tree inspect,
mixed knowledge/source inspect, removed-source whole-batch rejection, not-found and
malformed-input errors. The runner validates each against its matching X-1 **command**
schema and records `product-reading-responses.json`. Injected `CLI_RESPONSE` receipts
remain separately labelled and cannot satisfy this coverage. The earlier TK-11/TK-14
receipts cover only their then-current knowledge inspection; their historical limits
remain attached to those revisions. The transport collector accepts a real
`check {}` with empty errors and cache initialization; malformed input must fail
before initialization. TK-19 lifecycle acceptance is in `tests/check_lifecycle.rs`.

Use Python 3.11+ with the development dependency `jsonschema==4.26.0` prepared in
an isolated environment; it is not a product dependency. The following commands
are retained as historical TK-11/TK-12 invocations; their environment paths belong
to the worktree that captured those receipts:

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

## TK-14: load the small source-linked corpus

The fixed corpus has three knowledge elements, two normalized source addresses
(whole file and line 2), and five edges. It is loaded by real CLI change detection;
no SQL seed, library refresher or precomputed vectors are used. Templates become
Markdown only in the disposable repository. The source tree is committed first;
`inspect` later returns a different tree containing knowledge files.

From this project root, build `cargo build --locked --offline --bin letopis`,
then run this example. Keep the temporary repository path printed in the output.

```sh
python3 - "$PWD" "$(mktemp -d)" "$PWD/target/debug/letopis" <<'PY'
import json, os, shutil, subprocess, sys
from pathlib import Path
project, repo, binary = map(Path, sys.argv[1:])
fixture = project / 'tests/fixtures/markdown/thin_slice'
env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
def git(*args):
    return subprocess.check_output(['git', '-C', str(repo), *args], env=env).decode().strip()
git('init', '-q')
git('config', 'user.name', 'Fixture')
git('config', 'user.email', 'fixture@example.invalid')
(repo / 'src').mkdir()
shutil.copyfile(fixture / 'cache.rs', repo / 'src/cache.rs')
(repo / '.gitignore').write_text('.letopis/\n')
git('add', '.gitignore', 'src/cache.rs')
git('commit', '-qm', 'source basis')
tree = git('rev-parse', 'HEAD^{tree}')
(repo / 'knowledge').mkdir()
for stem in ('cache', 'flow'):
    text = (fixture / (stem + '.md.in')).read_text().replace('__SOURCE_TREE__', tree)
    (repo / 'knowledge' / (stem + '.md')).write_text(text)
response = subprocess.check_output([str(binary.resolve()), 'search'], cwd=repo, env=env,
    input=b'{"query":"CacheScope","limit":10}')
print(json.dumps({'repository': str(repo), 'source_tree': tree,
                  'search': json.loads(response)}))
PY
```

From the printed repository, pass `{"element_ids":["knowledge/cache.md",
"knowledge/flow.md#reuse"]}` to the same binary's `inspect` stdin. Read its
returned snapshot with `git show <snapshot>:knowledge/cache.md`; the source
basis is read separately with `git show <source_tree>:src/cache.rs`.
No model configuration is required for this FTS route. With a supported config,
the existing `embed --drain` computes missing vectors through the real provider.
TK-14 automatic tests use a controlled loopback Ollama HTTP fixture.

TK-15 should replace the fixed input set with its seeded target-scale corpus and
use this same foreground CLI loading route. This example does not establish
100k-item loading speed, RAM use or a real model's quality.

The runner records `thin-slice.log` and `thin-slice-responses.json`: seven required
actual-product cases cover loading, inspection, detached completion, provider
failure, invalid-file isolation/current rejection, and a text edit during HTTP.
The collector rejects missing cases, invalid command schemas, wrong exits and
wrong error classifications. The tests additionally observe source metadata and
persisted vectors through read-only APIs, never by seeding the projection.
Crash/rollback coverage comes separately from the current private document-service
process driver; this is not an actual-CLI crash checkpoint or a power-loss test.

TK-14's initial portable agent reference is [skills/letopis/SKILL.md](../../skills/letopis/SKILL.md).
It is supplied explicitly for the fresh-session smoke; installation/automatic
discovery in every agent runtime is not claimed. Baseline without the skill and
separate writer/reader sessions are retained alongside the acceptance evidence.

```sh
.artifacts/tk14/python-env/bin/python -B -m unittest discover -s tests/cli -p test_runner.py
.artifacts/tk14/python-env/bin/python -B tests/cli/run.py --platform host --evidence .artifacts/tk14/host-NEW
.artifacts/tk14/python-env/bin/python -B tests/cli/run.py --platform linux --evidence .artifacts/tk14/linux-NEW
```

Prepare Python 3.11+ and `jsonschema==4.26.0` before running these checks. Use
new evidence directories. The runner includes `make verify`; do not repeat it
on an unchanged accepted tree just to obtain a second identical log.

## TK-24 mixed inspection and agent workflow

The current `cli_reading` cases require metadata for a knowledge element, a local
source range, a declared missing local source, and an opaque remote source in one
real binary response. They check exact addressed hashes, duplicate normalization,
one retained Git tree, absent secret payloads, and whole-batch failure for a
source whose last current declaration was removed. The collector requires the
`inspect_mixed` and `inspect_removed` product receipts and validates each response
against the selected InspectOutput success/error schema. The accepted guide is
[skills/letopis/SKILL.md](../../skills/letopis/SKILL.md); TK-14's prior smoke
and receipts remain historical evidence of its older guide.

Run the deterministic no-provider workflow against a built binary. Use a new
evidence directory, outside the source tree if it cannot be written there:

```sh
python3 -B tests/cli/tk24_workflow.py --binary /absolute/path/to/target/debug/letopis --evidence /private/tmp/tk24-workflow-NEW
```

`workflow.json` contains every Git/CLI argv, input, exit, stdout and stderr,
source-basis and reading-tree OIDs, plus real-index and HEAD before/after checks.
The repository under the evidence directory is disposable. The script covers
write/check/search/traverse/mixed inspect and Git reads, a source edit and
`source_changed`, isolated temporary-index basis creation with addressed-byte
comparison, a dangling incoming link after rename and its repair. It establishes
CLI behavior for fixed bytes; it does not establish a real agent pilot, paid
model quality, resource targets, handled cancellation or other platforms.

## TK-20 source-service handoff

The 2026-10-25 [approved source contract](../../docs/superpowers/specs/2026-10-25-tk-20-source-identity-design.md)
is implemented through `SourceReader` with the caller's pinned `StorageRead`.
HTTP/HTTPS identities preserve original spelling/query/fragment and return unknown,
unfetched bytes without network or local-file reads. Missing local bytes return
`hash=None` and `SourceNotFound`; storage/unsafe-read failures remain typed errors.
At the TK-20 checkpoint, source CLI inspect remained TK-24 work; the current
mixed-inspect acceptance is above. Historical comparisons remained TK-21. No new CLI
fields or source flags are exposed here. Existing `check` refresh can reconstruct
the cache; CLI `reindex` is still pending.

Existing caches require the explicit, quiescent cache-only rebuild described in
[development guidance](../../docs/development.md#tk-20-явная-пересборка-кэша-при-обновлении).
This upgrade preserves canonical files, vectors, configuration and lock files.
Focused proofs: `tests/source_addresses.rs`, `tests/source_projection.rs`,
`tests/source_reading.rs`, `tests/source_lifecycle.rs`, and `tests/test_source_contract.py`.
Their service acceptance does not establish later CLI consumer behavior or G-1/G-2.

## TK-25: full reindex

The host/Linux runner includes `reindex_command`: full reparse of unchanged files,
strict empty input and safe errors, invalid-owner preservation, actual Git checkout/reset
with changed contents/membership, observational status after cache deletion, vector reuse
while embed.lock remains held, and author basis/index/HEAD preservation. Actual
`PRODUCT_REINDEX` records are checked against the reindex command schema and retained
in `product-reindex-responses.json`; injected handler receipts cannot replace them.
The transport collector also checks real reindex success and selective-input rejection.

Reindex process/SIGINT/WAL and late-error schedules run in the root library tests.
These prove rollback under native signal death; the handled `cancelled` JSON protocol
remains TK-29. No physical corruption repair or model-quality/pilot verdict is implied.
