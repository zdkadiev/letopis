# Supplementary campaign preparation

2026-11-17. HEAD `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. Round-two static gate permits bounded validation only; no baseline or profiling approved. Compilation is not a runtime observation.

## Observer

Built using existing `tests/performance/contention.py::build_observer` with Python 3.12, target `/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized`, and new evidence `evidence/native-observer-20261117-v1/`:

```sh
/opt/homebrew/bin/python3.12 -B -c 'import sys; from pathlib import Path; sys.path.insert(0,"tests/performance"); from contention import build_observer; build_observer(Path.cwd(), Path("/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-profile-20261117-0046a6d-symbolized"), Path("research_notes/Letopis comprehensive performance profiling/evidence/native-observer-20261117-v1").absolute())'
```

Helper runs `cargo test --release --locked --offline --test target_corpus --no-run --message-format=json`, setting `CARGO_TARGET_DIR` as above and `CARGO_PROFILE_RELEASE_DEBUG=1`. It inherits environment except `GIT_`, `LETOPIS_`, `OPENAI_`, `OLLAMA_`; sets `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, `TZ=UTC`. Other inherited build variables were not exhaustively captured. Rust `1.95.0`, `aarch64-apple-darwin`. Cargo diagnostics are in `cargo.log`; original `build.json` preserves Cargo-produced executable path.

Actual artifact copied to [`target_corpus-observer`](evidence/native-observer-20261117-v1/target_corpus-observer); [`archived-build.json`](evidence/native-observer-20261117-v1/archived-build.json) points to copy and retains original path and receipt. SHA-256 `ccc930977d0e9fd430fc55287072cae618326ab9c6eb277041d9a708dd1dcd68` matches original and archived binary. All 130 distinct recorded product, support, and entrypoint input hashes verified after build. No pre-existing candidate was assumed valid.

Observer `observe_existing_corpus` uses product `open_storage` and `StorageRead` under a pinned cache read transaction, validates cache/vector schema and WAL, identity, configured/stored dimensions, finite vector lengths, source/knowledge/edge counts and vector-key coverage. It writes only a new receipt, performs no inference or vector writes. `tests/support/target_corpus.rs:206-210,232-245` compares configured versus stored dimensions, not a hard-coded 32; driver `verify_native_coverage` requires 32 for its mock. This is source inspection, not runtime certification.

## Fixtures and inventory

```sh
/opt/homebrew/bin/python3.12 -B 'research_notes/Letopis comprehensive performance profiling/tools/profile.py' prepare --evidence '/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/supplementary-prepare-20261117-v1' --sizes 0,100,1000,10000 --include-real
```

No `--stress-100k`: canonical stress was already measured separately. Prepare creates independent local Git inputs and manifests only. Scratch: `/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode/letopis-research-zdikjzaj`. Thirteen fixtures and 159 distinct cases: [manifest](evidence/supplementary-prepare-20261117-v1/manifest.json), [all case IDs and default planned counts](evidence/supplementary-prepare-20261117-v1/case-inventory.md), [post-prepare hash verification](evidence/supplementary-prepare-20261117-v1/preparation-verification.json). All 1,050 immutable fixture entries, fixture digests, case digest, harness and four infrastructure hashes matched. Sparse and variant workload is supplementary, not canonical. Real snapshot copies bounded tracked content; historical bases are not repaired.

Driver SHA-256 `89cb6192138120a24e09b7b411be29cd3e2fa33fb6974bfed9f8edcaf610c1fb`; cases SHA-256 `c77a9198ca580576fbcc0ea252a66771fb3f97d68eb59d0e4530f0af3a02d0e3`.

## Runtime gate and subsequent selection

P0 validation selects only requested empty lifecycle, 100-node public operations, explicit enabled drain and vector-only canary. Each case runs once, with per-call timeout 120 seconds and aggregate stage budget at most 600 seconds. Record each success/failure/unavailable and stop on integration failure. No performance claims follow.

Bounded validation completed: [15 case results and semantic checks](evidence/supplementary-prepare-20261117-v1/smoke-validation.md); [raw rows](evidence/supplementary-prepare-20261117-v1/measurements.jsonl); [exact commands and per-case status](evidence/supplementary-prepare-20261117-v1/smoke-control/). All 15 succeeded; no case repeated. Explicit drain native observer confirmed 100 covered nodes at 32 dimensions. Canary input text lacks the query token, expected ID returned and query made one loopback request; no independent raw FTS query was captured. Sampler and full matrix remain unrun. Runtime ownership released for coordinator's semantic gate decision.

After a separate semantic approval, recommend finite batches: compact 100-node command/flag/help and traversal coverage once; representative 1,000-node graph-shape and source cases, then selected 10,000-node scaling cases, rather than a full cross-product. Keep default 20 measured cheap or 3 measured expensive, with first-observed and cheap conditioning separately; if budget requires reduction record requested/actual n and reasons. Full matrix would schedule 2,136 baseline slots and is not proposed.

Initial free space `df -h`: 57 GiB. Prepared scratch 18 MiB; preparation evidence 228 KiB; observer evidence 4.6 MiB. Requested soft combined nonbuild guard is 4 GiB if >=20 GiB free and post-observation RSS stop 1 GiB; frozen driver independently enforces a stricter 2 GiB combined scratch/evidence soft guard. Neither is a continuous hard ceiling. Do not silently change driver during gate.
