# Baseline build and samply validation — 2026-11-17

## Outcome

Both builds succeeded at exact requested revision **`0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`**. Tracked working tree was clean before and after compilation. No Cargo/source changes, installs, external provider calls, security changes, or commits.

Samply 0.13.1 successfully recorded the symbolized CLI without elevation: **127 usable stack observations**, two threads, one CLI PID (`50497`), **125 resolved Letopis function names**. This establishes usable launch-mode CPU sampling and Rust function symbolization on this host, **not benchmark results or comprehensive source-line fidelity**. Full machine-readable commands, environment allowlist, versions, file sizes, hashes, native linkage, executable UUIDs, and outcomes: [build_metadata.json](evidence/build_metadata.json).

## Executables for measurement runner

### Durable archive handoff (supersedes temporary paths below)

Both binaries, symbolized dSYM, 116 product input files, and original metadata copy are archived under:

```text
research_notes/Letopis comprehensive performance profiling/evidence/build-archives-20261117-0046a6d-v1/
```

- Stock harness receipt: [`stock_release/build.json`](evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json), truthful `release_debug: "0"`.
- Symbolized harness receipt: [`symbolized_release/build.json`](evidence/build-archives-20261117-0046a6d-v1/symbolized_release/build.json), truthful `release_debug: "1"`.
- Each receipt points to its adjacent `product/letopis`; symbolized product also includes `letopis.dSYM`.
- Existing `scenarios.read_product_inputs`, `hash_file`, `verify_new_output`, `write_json`, `verify_build`, `compare.verify_manifest`, and `run.read_build` owners composed for archiving/verification. Both receipts accepted actual current clean source and executable hashes; `run.read_build` accepted `--baseline` and wrote matching receipts without compiling or executing workloads.
- [Archive validation](evidence/build-archives-20261117-0046a6d-v1/archive_validation.json) records results. Original `evidence/build_metadata.json` preserved byte-for-byte, SHA-256 `6f915ede08d404679a9f9541d1b02ce5e8052a18080be60fcc37a0ba7f523bd7`.
- These are **post-build archive receipts**, not compiler attestations or invented original Cargo builder output. `compare.verify_frozen_build` hardcodes debug1 and original builder receipts; neither archive is labeled with that schema. Stock cannot honestly satisfy its debug1 requirement. Both are compatible with requested `scenarios.verify_build` and canonical runner `--baseline`.
- Actual sibling remains **debug1**, not draft spec debug2; no explicit stripping override was applied. Existing successful symbolization is sufficient; no rebuild performed.

### Stock locked release

```text
/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-stock/letopis
```

- Copied byte-for-byte after successful requested build from `/Users/zdkadiev/Repos/letopis/target/release/letopis`; use private copy to avoid later root-target overwrites.
- SHA-256: `77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`
- Size: **9,784,672 bytes**.
- Command, repository root:

```sh
cargo build --release --locked --offline --bin letopis
```

Cargo reported optimized release completed in **14.09 s** (incremental reuse permitted by Cargo). This is a build log value, not clean-build benchmark.

### Symbolized release sibling

```text
/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized/release/letopis
```

- SHA-256: `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`
- Size: **11,195,720 bytes**.
- Only build-profile override: `CARGO_PROFILE_RELEASE_DEBUG=1`. Same stock release optimization policy; **not** custom `dist` profile.

```sh
env \
  CARGO_TARGET_DIR=/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized \
  CARGO_PROFILE_RELEASE_DEBUG=1 \
  cargo build --release --locked --offline --bin letopis
```

Cargo reported optimized + debuginfo completed in **32.45 s** using separate target directory. No concurrent build or benchmark was launched by this worker.

Cargo did not initially emit adjacent `.dSYM`; samply nevertheless resolved names using available build artifacts. After capture, a portable matching symbol bundle was assembled without changing executable bytes:

```sh
xcrun dsymutil --out \
  /private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized/release/letopis.dSYM \
  /private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized/release/letopis
```

Exit 0; executable and dSYM UUID both **`FFEBF8EC-A110-37A6-AA8B-747F67DAEA21`**. DWARF file is 52,386,473 bytes, SHA-256 `7b00d4dcb28b34c7cdb9ff5652186b901e8718a466382b7c8fc16f4ca77b1618`. Preserve executable and adjacent dSYM together, or entire separate target directory. Raw captured symbol sidecar is preserved under evidence.

## Build identity, flags, native stack

- `rustc 1.95.0 (59807616e 2026-04-14)`, host `aarch64-apple-darwin`, LLVM **22.1.2**.
- `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`.
- macOS **26.6.2 / 25G83**, arm64, Mac16,7, 14 physical / logical cores, 48 GiB RAM.
- No ambient `RUSTFLAGS`, encoded Rust flags, or `CARGO_PROFILE_*` overrides detected. No ancestor/home Cargo config files discovered. Relevant environment allowlist and input hashes recorded in metadata; no secrets or full environment dump.
- Root manifest defines `profile.dist`, **not** `profile.release` overrides. Stock Cargo release defaults apply: optimization level 3, no LTO override, default 16 codegen units, unwind panic strategy, no explicit stripping, no explicit frame-pointer override. These values describe Cargo configuration, not a captured per-crate rustc invocation trace.
- Bundled SQLite and pinned vec1 C build come from [Cargo.toml](../../Cargo.toml) and [build.rs](../../build.rs); vec1 source hash recorded.
- Both `otool -L` results list only system dynamic dependencies: Security.framework, CoreFoundation.framework, libiconv.2.dylib, libSystem.B.dylib. No external SQLite dylib listed. This confirms observed dynamic linkage, not a substitute for the project's full native compatibility runner.
- `--help` and `--version` passed on both binaries; outputs identical. Version: **`letopis 0.3.1`**. Stock baseline ran no workload commands beyond these.

## CPU-profiler smoke and actual limitations

### Help-only attempt: insufficient evidence

`samply record --save-only --no-open --rate 1000 --iteration-count 100 --unstable-presymbolicate --output <evidence>/samply-help-smoke.json.gz <symbolized-binary> --help`

Exit 0, recorded 100 process/thread entries, **zero total/usable samples**, no Letopis symbols. Retained as **insufficient_samples**, not valid CPU profile or proof of Rust symbolization. Wrapper elapsed 3.018 s is not CLI latency. [Receipt](evidence/samply-help-smoke-receipt.json), [validation](evidence/samply-help-smoke-validation.json).

### Disposable workload: usable symbolized capture

Used existing fixture generator via `tests/performance/scenarios.py::prepare_fixture`, not a new reusable harness:

```text
/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-smoke-fixture/repository
```

300 synthetic elements, 2,000-byte fragments, 30 elements per Markdown file, deterministic seed 65, explicit disabled embeddings. No real or fake provider request needed for this smoke. Symbolized CLI `init` succeeded on owned fixture; `index --full` was sampled once with `{}` stdin:

```sh
samply record --save-only --no-open --rate 1000 \
  --unstable-presymbolicate \
  --output /Users/zdkadiev/Repos/letopis/research_notes/'Letopis comprehensive performance profiling'/evidence/samply-index-smoke.json.gz \
  /private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized/release/letopis \
  index --full
```

Executed with fixture as cwd and explicit JSON stdin using `subprocess`, not the main repository. Exit 0; product returned publication revision 1, 10 updated files, zero invalid files. Wrapper duration 1.438 s includes samply startup/finalization and is **not command latency**.

- **127 total and usable stack observations**, two threads, one recorded CLI PID.
- Observed CPU delta **120,540 µs**, not full-workflow wall duration.
- **125 resolved Letopis function names** observed in sampled stacks, including `letopis::commands::run_index`, `letopis::execute_request`, `DocumentRefreshService::publish_current`, and storage transaction methods.
- Inclusive costs overlap; no wall-time percentage, stable bottleneck, allocation attribution, or overhead estimate claimed.
- Samply succeeded: **macOS sample fallback deliberately not attempted**. No explicit `--pid` attach-mode test; successful launch-mode collection is sufficient for recommended runner strategy.
- No background worker coverage, actual provider exercise, source-line fidelity audit, allocation-history capture, or benchmark matrix performed.

[Capture receipt](evidence/samply-index-smoke-receipt.json), [raw profile](evidence/samply-index-smoke.json.gz), [symbol sidecar](evidence/samply-index-smoke.json.syms.json), [stack validation](evidence/samply-index-smoke-validation.json).

### Tooling hiccup

First stack-summary import used system `python3` and failed because its version cannot evaluate `Path | None` annotations. No profile data changed. Re-ran successfully with existing `/opt/homebrew/bin/python3.12 -B`; use that interpreter for performance harness. One tool invocation was transiently cancelled by location shutdown; read-only summary rerun succeeded.

## Handoff / lightweight next step

CPU-heavy compilation ownership is released: both Cargo builds and dsymutil have finished; this worker has no running build, sampler, provider, or workload.

Next lightweight runner inspection, no product execution or compilation:

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/run.py --list-scenarios
```

Next measurement worker should consume durable archive receipts above, **not automatically rebuild**. Original [metadata](evidence/build_metadata.json) remains preserved as historical build evidence, not mislabeled as a drop-in harness receipt.

Existing harness covers disabled/synthetic and deterministic-provider scenarios, not automatically all current commands/flags. Preserve stock and symbolized measurements as separate build identities. No installs or security approvals currently block CPU profiling; heap allocation chronology remains separate tooling gap.
