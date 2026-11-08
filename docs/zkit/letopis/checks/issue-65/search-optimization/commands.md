# Search optimization commands

Run from `/Users/zdkadiev/Repos/letopis`. The populated synthetic fixture and pinned
executables below already exist locally; do not regenerate or reembed it. Ollama
must be available on localhost with `embeddinggemma:300m-qat-q4_0`. Measurements
cover this host/corpus only. Run each experiment serially with no owned build,
test, drain or profiling operation in parallel.

## Actual-root verification and release build

These are the completed verification/build commands, with logs linked from the
[report](report.md). Rebuilding the mutable Cargo output does not replace the
captured measurement executable.

```sh
cd /Users/zdkadiev/Repos/letopis
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 make verify V=1
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --locked --offline
```

## Product commands on the populated fixture

These invoke the measured release. Search accepts JSON on stdin. This broad
hybrid request still observes the full candidate set before the final limit.
Check/status have not been remeasured by this search-only change; earlier
measurements are in the [native follow-up](../native-followup/report.md).

```sh
task_root=/Users/zdkadiev/Repos/letopis
search_binary="$task_root/.artifacts/issue-65/search-optimization-2026-11-06/product/letopis"
search_fixture="$task_root/.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository"
export NO_PROXY=127.0.0.1,localhost
export no_proxy=127.0.0.1,localhost

(
  cd "$search_fixture"
  "$search_binary" search <<<'{"query":"case 000000","limit":5}'
)
(
  cd "$search_fixture"
  "$search_binary" check
)
(
  cd "$search_fixture"
  "$search_binary" status
)
```

## Validated ordinary measurements

The archived [comparison driver](run-final-comparison.py) performed three
alternating hybrid pairs and five separate narrow FTS pairs. It deliberately
refuses an existing campaign output. Its exact successful results and manifests
are archived; do not remove them to rerun that campaign.

The following runs the same one-call measurement/oracle in a fresh directory.
It captures raw output, validation, binary/source identities, counts and wait4
resources. Setup and the executable help probe are outside the measured call.
The caller should warm the query model before timing when comparing releases;
the original outside-timer warmup is recorded in comparison-model-warmup.json.

```sh
cd /Users/zdkadiev/Repos/letopis
search_trial=$(mktemp -d "$PWD/.artifacts/issue-65/search-repeat-XXXXXX")
/opt/homebrew/bin/python3.12 -B \
  docs/zkit/letopis/checks/issue-65/search-optimization/measure-search.py \
  --binary .artifacts/issue-65/search-optimization-2026-11-06/product/letopis \
  --source "$PWD" --output "$search_trial/hybrid" \
  --label after --phase measured --mode hybrid
/opt/homebrew/bin/python3.12 -B \
  docs/zkit/letopis/checks/issue-65/search-optimization/measure-search.py \
  --binary .artifacts/issue-65/search-optimization-2026-11-06/product/letopis \
  --source "$PWD" --output "$search_trial/fts" \
  --label after --phase measured --mode fts
```

For a baseline call, use binary
`.artifacts/issue-65/2026-11-06-native-followup/product/letopis`, source
`.artifacts/issue-65/2026-11-06-native-followup/source`, label `before`, and a fresh
output path. Verify binary and all 106 source hashes against baseline-build.json
or after-build.json before comparing; the one-call driver records identities,
while the archived comparison driver additionally enforces those pins.

## Genuine Samply recording

Run separately from ordinary timing. Save-only mode starts no server/browser and
uploads nothing. This command profiles only the owned search command. The archived
recording uses these options; new recordings must have fresh output paths.

```sh
task_root=/Users/zdkadiev/Repos/letopis
search_binary="$task_root/.artifacts/issue-65/search-optimization-2026-11-06/product/letopis"
search_fixture="$task_root/.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository"
search_profile=$(mktemp -d "$task_root/.artifacts/issue-65/search-samply-XXXXXX")
(
  cd "$search_fixture"
  NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost \
    /Users/zdkadiev/.cargo/bin/samply record \
    --save-only --rate 1000 --unstable-presymbolicate \
    --output "$search_profile/search.json.gz" \
    "$search_binary" search <<<'{"query":"case 000000","limit":5}'
)
```

The independent method reports preserve parser microbench, mutation, SQL probe,
and ranking-counterexample commands. Their temporary clones/targets remain under
`.artifacts/issue-65/search-optimization-2026-11-06/`; text sources, patches and
receipts are archived alongside this report. Database/binary/build directories
are local ignored artifacts, not copied into the documentation archive.
