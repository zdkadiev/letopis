# Drain-scale adjunct preparation — drain-only plan ready for review

The six-case data-only proposal is preserved in [`evidence/drain-scale-20261117-prep-v1/`](evidence/drain-scale-20261117-prep-v1/). The coordinator narrowed scope to **three drain cases** and postponed search. The one-off launcher and selected casebook are prepared for review; no fixture generation or execution is authorized until explicit ownership transfer.

## Drain-only selection and launcher

[`drain-cases.json`](evidence/drain-scale-20261117-prep-v1/drain-cases.json) selects exactly `sparse-2.mock-drain`, `sparse-100.mock-drain` and `sparse-1000.mock-drain`. [`drain-selection.json`](evidence/drain-scale-20261117-prep-v1/drain-selection.json) freezes each case hash, the full selection hash, driver and producer hashes, dimensions, repeats and timeout. The original six-case proposal remains historical and unchanged.

[`launcher.py`](evidence/drain-scale-20261117-prep-v1/launcher.py) has separate `--prepare` and `--execute` phases. `--prepare` invokes the unmodified driver `prepare`, preserves the generated original catalog and manifest in the new campaign, and freezes the selected catalog and matching manifest **before validation**. It records launcher, driver, selected cases, product executable and full build receipt, compilation metadata, observer binary and receipt, product revision and all generated fixture hashes. The existing driver's immutable fixture and source/build gates remain active. The driver also generates seven supplementary 1000-node variants and a canary during preparation; these are not selected for execution.

`--execute` refuses a second start, verifies the pins again and, case by case, invokes the existing driver for one validation and then three ordinary measured repeats, with a 120-second per-command timeout and 1800-second stage budget. After each invocation it independently requires N knowledge nodes, N distinct search hashes and queue N before drain; queue zero, no failures and exactly N provider **items** after drain; plus native current-configuration N covered, zero missing/eligible/invalid and 32 dimensions. It does not assume one HTTP request. An invalid, timed-out, missing or incorrectly counted result stops subsequent stages; partial raw receipts remain. Driver resource guards and plan accounting are unchanged.

The stricter adjunct checks run after the driver writes its receipts. A row may therefore retain the driver's `is_valid=true` even when the adjunct rejects an additional count. Treat absence of `adjunct-execution-complete.json` as incomplete and inspect the logs and raw receipts; do not infer adjunct success from the driver's validity field alone.

After explicit ownership transfer only, from the repository root, choose a **new** absolute path directly under this research evidence directory:

```sh
PY=/opt/homebrew/bin/python3.12
ROOT="$PWD/research_notes/Letopis comprehensive performance profiling"
LAUNCHER="$ROOT/evidence/drain-scale-20261117-prep-v1/launcher.py"
RUN="$ROOT/evidence/drain-scale-20261117-execution-v1"
"$PY" -B "$LAUNCHER" --prepare --evidence "$RUN"
# Inspect the frozen manifest, cases, adjunct-pin.json and prepare log first.
"$PY" -B "$LAUNCHER" --execute --evidence "$RUN"
```

Neither command was run here. `--prepare` creates fixtures, including local Git repositories; `--execute` runs product and the native observer. The launcher does not compile, profile, alter the original campaign, or automatically retry interrupted execution. Each stage's log records its driver arguments and output. Actual runtime compatibility, counts and timings remain unmeasured until execution; static checks are not a measurement approval.

## Original search-scope finding — postponed, not a drain blocker

The frozen driver checks `cases.json` against `manifest["cases_sha256"]` before execution and checks each complete fixture input inventory. A separate catalog can therefore be constructed and hashed with its own matching manifest before its first validation, without altering the active campaign. The proposal clones only the two reviewed 100-node case records, changing their ID and fixture ID for 2, 100 and 1000 nodes. Each fixture must independently contain exactly the requested number of unique text hashes; the count must be verified, not inferred from case names.

However, catalog data alone cannot satisfy native coverage for the *actual search sample*. `prepare_state` drains the search fixture, but `run_sample` invokes `observe_native` only for the measured `embed --drain` command; the search fixture is deleted when `run_sample` returns. The search validator only requires at least one provider request and no recorded query error; it does not require exactly one request or independently verify native vector coverage. A separate earlier drain is not evidence about the search's own disposable database.

The previously considered search-specific orchestrator is not implemented. Search, query HTTP counts and populated-search native coverage are outside this drain-only selection.

## Frozen proposal

[`casebook.json`](evidence/drain-scale-20261117-prep-v1/casebook.json) and [`casebook-provenance.json`](evidence/drain-scale-20261117-prep-v1/casebook-provenance.json) contain exact case records, individual hashes, full casebook hash and producer/driver/catalog hashes. These are case hashes, not fixture or build identity, and do not claim runtime validation. Generator only reads the frozen catalog and driver:

```sh
/opt/homebrew/bin/python3.12 -B 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v1/casebook.py'
```

Output files are exclusive-create and already exist; do not rerun over them. The historical casebook contains six cells, but only the three drain cells above are selected. Default mock is 32 dimensions. The existing case fields support `delay_ms=50`, but no delay cases are selected; delay remains unmeasured. No model quality or semantic ranking claim follows.

Archived product build and observer paths and observer provenance are documented in [supplementary preparation](supplementary_preparation.md). Those artifacts were not executed or built here.

## Checks

```sh
/opt/homebrew/bin/python3.12 -B -m unittest discover -s 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v1' -p '*_test.py' -v
```

Four lightweight tests passed: [`launcher-tests.txt`](evidence/drain-scale-20261117-prep-v1/launcher-tests.txt). A missing native `missing_nodes` value was first demonstrated to pass incorrectly, then rejected and retested. [`mutation.txt`](evidence/drain-scale-20261117-prep-v1/mutation.txt) records a detected change of the 1000-node scale; [`launcher-mutations.txt`](evidence/drain-scale-20261117-prep-v1/launcher-mutations.txt) records detected queue and selection mutations, with original launcher bytes restored. [`static-pins-check.txt`](evidence/drain-scale-20261117-prep-v1/static-pins-check.txt) records read-only verification of the observer and its recorded inputs, current product source against the 116-input receipt, and the selection. No Rust, benchmark, profile, native observer or heavy fixture execution was performed.
