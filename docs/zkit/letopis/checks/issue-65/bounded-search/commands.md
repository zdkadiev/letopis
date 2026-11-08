# Bounded hybrid search commands

Run from `/Users/zdkadiev/Repos/letopis`. Historical measurement directories must
not be overwritten. These commands use the owned current-schema 100k fixture,
not the repository's older knowledge cache. No global binary was replaced.

## Verified build and native checks

```sh
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 \
  /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host \
  --evidence .artifacts/issue-65/bounded-search-2026-11-07/host-storage
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 \
  cargo build --release --locked --offline --message-format=json
```

The first command includes full `make verify V=1`, strict Rustdoc, the explicit
real60-second provider deadline, native capabilities and copied standalone
executable/linkage checks. Its recorded output directory already exists.
For a new gate, make a fresh parent with `mktemp -d` and pass its absent `host`
child to `--evidence`.

## Search

```sh
bounded_root=/Users/zdkadiev/Repos/letopis
bounded_binary="$bounded_root/.artifacts/issue-65/bounded-search-2026-11-07/product/letopis"
bounded_fixture="$bounded_root/.artifacts/issue-65/bounded-search-2026-11-07/repository"
export NO_PROXY=127.0.0.1,localhost
export no_proxy=127.0.0.1,localhost
(
  cd "$bounded_fixture"
  printf '%s\n' '{"query":"case 000000","limit":5}' | "$bounded_binary" search
  printf '%s\n' '{"query":"case 000000"}' | "$bounded_binary" search
  printf '%s\n' '{"query":"case 000000","limit":1000}' | "$bounded_binary" search
)
```

Effective L is100 if omitted, otherwise min(positive limit,100). Each SQL channel
returns at most L knowledge hits; the union is at most2L, final response at mostL.
Filters apply inside that pool without refill. The last two requests therefore
use the same100 limit. Their output equivalence is checked separately from timing.

Captured executable SHA-256:
`117bee3d746af4946c0a3a52f2aa6e8bd972e06ba25d2e982b1fe27a73af53ba`.
Mach-O UUID:`519E4889-7285-3915-98A8-59BE598FA9D1`.

## Recorded performance campaign

```sh
/opt/homebrew/bin/python3.12 -B \
  .artifacts/issue-65/bounded-search-2026-11-07/benchmark.py
```

The captured driver reserves an exclusive new campaign, refuses existing output,
requires passed actual-root gates, checks binary/source/method/config/model pins,
validates against independent SQL ranking and freshness evidence, and keeps every
sample. It performs two separate screening calls, three alternating ordinary
pairs per limit5/100, then default/clamped smoke calls. Do not delete its evidence
to replay it. Preparation, SQL oracles, state checks and provider warmup are outside
CLI timers; caches are not controlled. Raw commands and dependency pins are in the
campaign manifest. Only the ordinary pairs contribute to medians.

## Genuine Samply

The recorded profile is a separate call after the ordinary campaign. A new
save-only recording can use a fresh directory:

```sh
bounded_profile=$(mktemp -d "$PWD/.artifacts/issue-65/bounded-samply-XXXXXX")
(
  cd "$bounded_fixture"
  printf '%s\n' '{"query":"case 000000","limit":100}' | \
    /Users/zdkadiev/.cargo/bin/samply record --save-only --rate 1000 \
    --unstable-presymbolicate --output "$bounded_profile/search.json.gz" \
    "$bounded_binary" search
)
```

Save-only opens no UI/server and uploads nothing. Profile wall time/RSS includes
the profiler wrapper and is not an ordinary CLI measurement.
