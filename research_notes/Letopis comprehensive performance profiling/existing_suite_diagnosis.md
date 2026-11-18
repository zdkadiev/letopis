# Existing performance-suite failure diagnosis

2026-11-17. Investigation only. No source/test/tool repair, real product execution, compilation or sampler invocation. Original failed suite output remains unchanged in [`evidence/harness-review-v1/round1-existing-tests.txt`](evidence/harness-review-v1/round1-existing-tests.txt).

## Established immediate cause

The suite error is `StopIteration` at `tests/performance/test_compare.py:98`, not an error inside the required-counter implementation.

Observed causal chain:

1. Failed-run log line1 records **before-side fake first publication timed out at5.006961s**; after side succeeded at0.057558s.
2. `_args` (`test_compare.py:419-422`) sets `initial_timeout=5`, `timeout=5`, `stage_seconds=60`.
3. `compare.run_stages` (`compare.py:139-154`) persists both first-publication attempts and explicitly returns if either validation is false.
4. Consequently `run_probe_series` and the intentionally failed counter are never reached. `build_campaign_summary` (`compare.py:311-320`) derives series only from actual rows, so its result legitimately contains first-publication series and **no measured series**.
5. The test first confirms overall `incomplete`, which succeeds for either failed initialization or failed counter. It then unconditionally asks for a measured series. With the observed initialization timeout, `next(...)` raises.

This explains the missing series directly from preserved log plus code; no attribution to the new supplementary harness is needed to explain this path. It does **not** prove why the fake launch exceeded5s.

## Product/build/sampler safety verified before targeted run

Read complete test and its helpers:

- `_freeze` (`test_compare.py:425-448`) patches `compare.build_release`; its replacement writes an executable **Python script**, not Rust output. Script body `_CLI` (`:451-463`) creates literal ordinary SQLite test tables and emits fixed JSON for60 knowledge/60 source/300 edge counts.
- `_record_fixture_profile` (`:373-379`) runs that same script without sampling. The test patches `profile.record_profile` and `profile.record_git_calls` around its campaign.
- `_record_failed_counter` (`:382-390`) runs only `python -c 'import time;time.sleep(60)'` under a0.01s collector deadline. It is a timeout-helper process, not a60-second benchmark.
- Fixture preparation uses local Git plus small text files. No Cargo build, native executable, real provider or sampler is needed on this path.

Runtime diagnostic guard allowed only **exact expected fake-script bytes**, that exact timeout helper, local nonremote Git and read-only OS metadata. Any unexpected native executable/build/sampler would have stopped before launch. Module paths were recorded and all resolve to existing `tests/performance` owners, not research `tools/profile.py`.

## One actual targeted UNIT execution

An in-memory diagnostic wrapper observed `subprocess.Popen`, measurement results and the returned campaign, without changing files or test assertions. Two initial guard preflights stopped at harmless `platform.platform()` metadata (`uname`, then `file`) **before any fake-product call**; their outputs remain in `evidence/existing-suite-diagnosis-v1/` and `v2/`. After explicitly permitting read-only platform metadata, the single actual fake-product unit execution ran:

```text
test_compare.ComparisonTests.test_failed_required_counter_marks_profile_campaign_incomplete
Ran 1 test in 1.700s
OK
```

Evidence: [`v3/targeted-output.txt`](evidence/existing-suite-diagnosis-v3/targeted-output.txt), [`v3/targeted-result.json`](evidence/existing-suite-diagnosis-v3/targeted-result.json), copied [`v3/targeted-campaign/`](evidence/existing-suite-diagnosis-v3/targeted-campaign/).

Observed:

- Before/after fake first publication:0.314210s/0.038228s; both validated.
- Both pair0 measured profiles were correctly invalidated by the intentionally timed-out required counter. Later two pairs validated. The test's expected measured-series failure counts were present.
- All20 fake-product launches passed exact-script-byte guard; observed `Popen` creation durations about0.00135–0.00263s. First full invocation still cost0.314s, so this run does not localize original slow time to `Popen`.
- Initial and final `SIGALRM` timers were both zero. No fresh-process evidence of leaked stage alarm.
- Load averages before/after:13.3911/8.2773/8.0537 on this host. Original failed suite did not capture comparable load/scheduling/launch-phase observations.
- No actual product/native observer/build/sampler was launched.

The failed entire suite was **not** rerun. The isolated pass does not replace that failed suite result or prove a flaky/unrelated-test diagnosis.

## Comparison with earlier success and hypotheses

Earlier whole-suite success in this session:94 tests,8 skipped,20.738s. Its first before-side fake publication was0.338754s and after side0.049851s. Latest failed suite:94 tests,8 skipped,82.333s. Several later before-side first publications were also slow (4.7462,4.6590,4.5787,4.5653,4.7606s), while corresponding after calls remained about0.03–0.06s.

That repeated **first-use-before versus reused-after asymmetry** is stronger evidence than simply observing a longer total suite time. Candidate explanations, still unproven:

| Hypothesis | Supporting evidence | Missing/disconfirming evidence |
| --- | --- | --- |
| Shared-host scheduling/I/O pressure or first-execution platform work pushes fresh fake script over fixed5s initialization deadline | Original measured5.006961s timeout, several other fresh before launches close to5s; isolated run returns below deadline | Original run has no scheduler/I/O/platform diagnostics; global host pressure alone does not explain all first-use asymmetry |
| Persistent monkeypatch/module contamination from new driver | Both codebases have a module named `profile`, worth checking | Existing suite was run in its own new interpreter with `-s tests/performance`; isolated module paths are correct. Original run did not record `sys.modules`, so historical pollution is not conclusively excluded; no evidence establishes it |
| Leaked prior test stage alarm | Campaign uses real `SIGALRM`; could affect control flow | Failure was recorded as a **5s per-call timeout**, not stage interruption; code restores alarm in `finally`; targeted before/after timer is zero. No affirmative leakage evidence |
| Counter/profile assertion itself failed | Test name concerns failed required counter | Original log never reaches measured phase; initialization gate explains absence before counter runs. Isolated required-counter path behaves as asserted |

**Conclusion:** immediate failure path is established: initialization timeout → intentional gate return → unconditional test lookup of absent measured series. Underlying reason for that run's >5s fake first publication remains undetermined. No source fix proposed or applied while review is active.

## Existing native observer inventory — metadata only

Entry point exists in `tests/target_corpus.rs:29-35`: ignored `observe_existing_corpus`, driven by `LETOPIS_CORPUS_REQUEST`. `tests/support/target_corpus.rs:68-105` reads existing config/cache/vectors, uses product/native vector storage and writes a new observation receipt; it does not initialize, reset or infer. Request includes expected knowledge/source/edge counts; schema versions4/1, current configuration identity and dimensions are checked.

Existing executable candidates found without running them:

- Latest debug: `/Users/zdkadiev/Repos/letopis/target/debug/deps/target_corpus-5055de60942736ab`,8,102,032 bytes,mtime2026-11-15T14:45:54Z, SHA-256 `d0f87f380fb306ac7bca860ab54056df2fb550583231de683455ef9cad6d8bc2`.
- Latest release by mtime: `/Users/zdkadiev/Repos/letopis/target/release/deps/target_corpus-5eafe7212e454bfd`,4,301,664 bytes,mtime2026-11-15T12:29:29Z.
- Four older release candidates also exist (November4–7).

Cargo fingerprint JSON exists for each. Full candidate paths/metadata/fingerprint payloads are preserved in [`observer-candidates.json`](evidence/existing-suite-diagnosis-v3/observer-candidates.json).

**Provenance limit:** no matching current-revision native-observer build receipt was found in the current profiling evidence or searched historical checks. Cargo fingerprint and mtime are not a full116-input/current-HEAD provenance receipt; no executable was run to claim schema/runtime compatibility. Existing `contention.build_observer` (`tests/performance/contention.py:300-319`) is the appropriate future build owner and records executable hash, complete product inputs, support inputs and entrypoint hash.

After exclusive profiling work releases ownership: either obtain a matching existing observer receipt from its owner, or build/archive that test executable with existing locked/offline no-run recipe:

```text
cargo test --release --locked --offline --test target_corpus --no-run --message-format=json
```

This is a **future** action, not executed here. Candidate existence alone is insufficient to certify reuse.
