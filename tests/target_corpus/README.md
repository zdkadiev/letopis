# Target corpus load and drain

This tooling measures whether the real `letopis` CLI can load and embed a
deterministic local corpus. The 100,000-node target is still pending controller
acceptance; the retained 60-node receipts are a smoke run only. These corpora
are excluded from usefulness pilots. They make no RAM, latency, quality or
fixed drain-time claim. Parallel drain proposal #49 is separate; this workflow
uses the current sequential `embed --drain` command.

## Build and discover the executables

Run from the repository root with locked, offline dependencies already
available. Discover executable paths from Cargo's JSON messages instead of
depending on Cargo's changing hashed test filename:

```sh
set -euo pipefail
TASK_TMP=$(mktemp -d "${TMPDIR:-/tmp}/tk15-paths.XXXXXX")
trap 'rm -rf "$TASK_TMP"' EXIT
cargo build --release --locked --offline --message-format=json \
  | python3 -c 'import json,sys; [print(m["executable"]) for line in sys.stdin if (m:=json.loads(line)).get("reason")=="compiler-artifact" and m.get("target",{}).get("name")=="letopis" and m.get("executable")]' \
  > "$TASK_TMP/product-paths"
cargo test --locked --offline --test target_corpus --no-run --message-format=json \
  | python3 -c 'import json,sys; [print(m["executable"]) for line in sys.stdin if (m:=json.loads(line)).get("reason")=="compiler-artifact" and m.get("target",{}).get("name")=="target_corpus" and m.get("executable")]' \
  > "$TASK_TMP/observer-paths"
PRODUCT=$(tail -n 1 "$TASK_TMP/product-paths")
OBSERVER=$(tail -n 1 "$TASK_TMP/observer-paths")
test -x "$PRODUCT" && test -x "$OBSERVER"
```

The product is the `letopis` binary; the observer is the executable generated
for the `target_corpus` integration test. Keep the selected files unchanged
through load, drain and observe: their SHA-256 digests are pinned on first load.
The path files live in a newly created private temporary directory; the exit
trap removes only that directory.

## Generate and validate

Every output directory must be new. The generator writes a bare Git fixture,
`corpus.json`, and isolated Markdown/source files; generation does not invoke
the product or a model. Resolve output paths to avoid symlinked ancestors.

```sh
OUT="$PWD/.artifacts/tk15/smoke-60"
python3 -B tests/target_corpus/generate.py --output "$OUT" \
  --nodes 60 --seed 15 --fragment-bytes 2000 --elements-per-file 30
python3 -B tests/target_corpus/validate.py "$OUT/corpus.json"
```

For the acceptance-sized corpus, use a different, absent output directory and
the same recorded seed and payload settings:

```sh
OUT="$PWD/.artifacts/tk15/target-100k"
python3 -B tests/target_corpus/generate.py --output "$OUT" \
  --nodes 100000 --seed 15 --fragment-bytes 2000 --elements-per-file 30
python3 -B tests/target_corpus/validate.py "$OUT/corpus.json"
```

The manifest records the fixture's source Git tree and input SHA-256 values,
counts, packing and exact UTF-8 byte statistics for marked knowledge fragments
and Markdown files. The source tree is captured before the generated knowledge
is committed into the fixture. The later product load creates its own knowledge
snapshot; the observer's `project_revision` and graph counts describe that
knowledge/inspect snapshot, not the earlier source-tree OID. The target is
100,000 knowledge nodes, 100,000 distinct
normalized source addresses and 500,000 distinct edges; each marked fragment
must be exactly 2,000 UTF-8 bytes. The default 30 elements per file yields 3,334
files and 10 elements in the last file. A node count alone does not prove the
payload-size target. Sources are repository-relative file/line addresses (for
example, `sources/shard-0000.txt#L1-L1`), not source files loaded as knowledge;
the corresponding shard is stored under `sources/` in the fixture.

## Load, drain, observe and resume

Set `CORPUS` to the generated manifest and preserve both executable paths. The
example config is optional: without it the runner pins its supported default
local Ollama config (`embeddinggemma:300m-qat-q4_0`, loopback port 11434, two
retries). An explicit config must have the same supported provider/model and an
HTTP loopback URL; paid APIs, proxy routes and model downloads are unsupported.
The model must already be installed locally. The runner records its digest and
config identity without making an inference request during load.

```sh
CORPUS="$OUT/corpus.json"
EVIDENCE="$PWD/.artifacts/tk15/load-100k"
python3 -B tests/target_corpus/run.py --corpus "$CORPUS" \
  --binary "$PRODUCT" --observer "$OBSERVER" \
  --evidence "$EVIDENCE" --phase load
```

Load uses the real CLI's empty-query `search` to initialize configured stores,
then records a backlog only if coverage is zero and eligible work is nonzero.
The first load durably writes `runner-pin.json` beside `corpus.json` and
`.letopis/config.json` inside the fixture repository. Do not edit or delete
these files or the product databases. If first initialization stops partway,
retry `load` only when the databases are absent or provably empty. For an
existing initialized database, the runner checks persisted identity using a
narrow SQLite read-only metadata guard before search; only empty first-init
metadata may proceed without identity. A mismatch or occupied identity-less
state fails closed before product refresh. Matching initialized state must also
pass the Rust observer under the embedding lock.
The observer requires the product to have initialized regular version-1 WAL
stores; load performs the product initialization before its first observation.

Run a bounded drain first if desired; `--max-seconds` is a watchdog, not a
product SLA:

```sh
EVIDENCE="$PWD/.artifacts/tk15/drain-100k-attempt-1"
python3 -B tests/target_corpus/run.py --corpus "$CORPUS" \
  --binary "$PRODUCT" --observer "$OBSERVER" \
  --evidence "$EVIDENCE" --phase drain --max-seconds 600
```

Continue interrupted or incomplete work with the same corpus, executables,
installed model and pinned config, and a new evidence directory. Never rerun
generation over the existing corpus or overwrite earlier receipts.
On timeout or handled interruption, the runner terminates and reaps only its
owned drain process group. Do not use broad process-name cleanup. A forced
`SIGKILL` of the runner or power loss may prevent the current phase from writing
its final receipt; use a fresh evidence directory to record the next phase.

```sh
EVIDENCE="$PWD/.artifacts/tk15/drain-100k-attempt-2"
python3 -B tests/target_corpus/run.py --corpus "$CORPUS" \
  --binary "$PRODUCT" --observer "$OBSERVER" \
  --evidence "$EVIDENCE" --phase drain
```

Observe without invoking a drain:

```sh
EVIDENCE="$PWD/.artifacts/tk15/observe-100k-final"
python3 -B tests/target_corpus/run.py --corpus "$CORPUS" \
  --binary "$PRODUCT" --observer "$OBSERVER" \
  --evidence "$EVIDENCE" --phase observe
```

Load exits successfully with status `backlog` and verdict `not_assessed`.
Drain or observe exits successfully with status/verdict `populated` only when
all current knowledge nodes have valid current vectors under the pinned
configuration. Incomplete or exhausted work exits nonzero and remains
`not_assessed`; zero eligible hashes does not imply population when nodes are
still missing. Each phase requires a new evidence directory. It retains
`runner-receipt.json`, `observer-request.json`, `observation.json`, and process
metadata; load writes `refresh-process.json`, drain writes
`before-observation.json` and `drain-process.json`. Process receipts contain
argv, stdin hex, exit/outcome, byte counts and stdout/stderr SHA-256 hashes,
not raw output or knowledge/provider payloads. The phase's stdout is one
hash-free summary JSON line (`phase`, `status`, `verdict`, `error`).

The retained smoke evidence under `.artifacts/tk15/task3-real-model-20261024-02/`
used 60 knowledge nodes, 60 sources and 300 edges. Its load receipt reported a
zero-covered backlog; its subsequent local-model drain receipt reported
60/60 covered and dimension 768. These receipts validate the small workflow,
not the 100,000-node acceptance run. This retained execution evidence is from
macOS only; it provides no Linux, full-target RAM, or machine-size result.
Report full-target success only from the controller's primary evidence and hand
that evidence off with the TK-16 results.
