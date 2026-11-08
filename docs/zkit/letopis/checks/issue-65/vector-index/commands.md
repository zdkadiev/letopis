# Exact vector index: commands

Run from `/Users/zdkadiev/Repos/letopis`. The captured release is immutable;
rebuilding `target/release` does not replace it. The original populated corpus
was preserved. Examples use the separate current-schema experiment copy.

## Completed verification and build

```sh
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 \
  /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host \
  --evidence .artifacts/issue-65/vector-index-2026-11-07/host-storage
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 \
  cargo build --release --locked --offline --message-format=json
```

The recorded host evidence path now exists and must not be reused. For a repeat,
create a fresh parent with `mktemp -d` and pass its absent `host` child. The host
runner includes full make verify and the explicit60-second provider deadline.
No Linux run is implied.

## Product initialization and search

Writable preparation automatically upgrades a recognized compatible none/cos
database once. It preserves vectors and identity. Already-flat opens need no
rebuild; status remains observational. The root's installed0.2 knowledge cache
is an older schema and was kept intact; these examples do not migrate that cache.

```sh
vector_root=/Users/zdkadiev/Repos/letopis
vector_binary="$vector_root/.artifacts/issue-65/vector-index-2026-11-07/product/letopis"
vector_fixture="$vector_root/.artifacts/issue-65/vector-index-2026-11-07/repository"
export NO_PROXY=127.0.0.1,localhost
export no_proxy=127.0.0.1,localhost
(
  cd "$vector_fixture"
  "$vector_binary" init </dev/null
  printf '%s\n' '{"query":"case 000000","limit":5}' | "$vector_binary" search
)
```

The captured executable SHA-256 is
`ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9`.
The existing fixture uses real local Ollama embeddinggemma vectors. No document
drain, model replacement or configuration reset is needed.

## Recorded comparison and new single-call measurements

The completed three-pair campaign was:

```sh
python3.12 -B .artifacts/issue-65/vector-index-2026-11-07/run-comparison.py
```

The [archived driver](run-comparison.py) intentionally refuses existing output,
reserves its campaign exclusively and requires the passed host gate. Do not delete
old evidence to rerun it. It uses the same copied fixture, restores the two native
modes outside timers, warms the query model, and checks all107 candidate inputs
and106 baseline inputs. Mode-setup failures have separate immutable receipts.

For a new after-only observation with the checked driver and a fresh output path:

```sh
vector_trial=$(mktemp -d "$PWD/.artifacts/issue-65/vector-repeat-XXXXXX")
/opt/homebrew/bin/python3.12 -B \
  docs/zkit/letopis/checks/issue-65/vector-index/measure-cli.py \
  --binary "$PWD/.artifacts/issue-65/vector-index-2026-11-07/product/letopis" \
  --source "$PWD" \
  --pin .artifacts/issue-65/vector-index-2026-11-07/product/after-build.json \
  --output "$vector_trial/search" --label after --model flat
```

A single call is not a before/after comparison. The driver refuses changed
product inputs and validates the five literal expected IDs, clean flags and
unchanged vector state. Full output equality across all pairs additionally checks
order, paths, coordinates and revision. Host/cache remain uncontrolled.

## Genuine Samply

The completed recording used [record-profile.py](record-profile.py), after ordinary
timing. An equivalent fresh recording is:

```sh
vector_profile=$(mktemp -d "$PWD/.artifacts/issue-65/vector-samply-XXXXXX")
(
  cd "$vector_fixture"
  printf '%s\n' '{"query":"case 000000","limit":5}' | \
    NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost \
    /Users/zdkadiev/.cargo/bin/samply record \
    --save-only --rate 1000 --unstable-presymbolicate \
    --output "$vector_profile/search.json.gz" "$vector_binary" search
)
```

Save-only starts no UI/server and uploads nothing. Keep recordings separate from
ordinary measurements. Native SQL/probe/backup commands and failed controls are
preserved in [native-probe](native-probe/README.md); never apply experimental SQL
rebuilds directly to a database being used by the application.
