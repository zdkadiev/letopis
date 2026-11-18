# Final scoped probe gate

**A. Drain cross-stage resource-stop fix: PASS for bounded runtime pilot.**
**B. Direct-root CPU adapter: PASS for bounded runtime pilot.**

Static source and saved handoff review only; no tests, product, observer, sampler or tool probes were run. This gate does not claim runtime compatibility or measured results. The coordinator's one-time closing ruling permits probes to start until **2026-11-17 20:00:00Z**, subject to exclusive ownership and the stated external guards.

## A. Drain stop fix

[Scoped diff](evidence/drain-scale-20261117-prep-v2/scoped.diff) checks newly persisted rows and setup receipts **after each driver stage and before the next CLI invocation**. A last valid row at or above 1 GiB, setup RSS stop, recorded resource-stop reason or resource/budget skip stops subsequent stages; missing or malformed rows and failed subprocesses also prevent continuation. The saved handoff reports nine pure tests and a detected exact-threshold mutation; these were not rerun. Existing driver still owns in-stage stops, and this is a post-observation soft guard, not a live RSS ceiling. [Handoff](drain_scale_stop_fix.md).

## B. Direct-root CPU

[Current adapter](tools/direct_root_cpu.py) verifies the full build/source receipt, copies and reverifies a private binary and dSYM, and requires the private library path and UUID in profile metadata. Product-library-only weighted functions are retained separately from all-library PID stacks. Profile-derived PIDs are explicitly not an OS exec trace. Missing sampled library or PID is insufficient and eligible for the finite diagnostic; conflicting identity is an error. A valid result requires at least 100 product-library stacks and positive resolved Letopis CPU.

Enabled search checks native current-configuration coverage before and after on the same disposable database. Drain requires N unique hashes, queue N→0, zero failures, N provider items and native N coverage at 32 dimensions. Config/help/version alone request 64 EOF iterations and validate output count; other cases execute once on fresh state. Sampler status is not represented as individual child exit status. Saved preparation reports eight pure tests and six detected mutations; these were not rerun. [Preparation and execution contract](direct_root_cpu_preparation.md).

The caller remains responsible for finite ordering, shared 600–900-second deadline, 120-second per-call limit, fresh evidence, and external checks of combined disk below 4 GiB and free space at least 20 GiB. These are soft controls; aggregate sampler RSS does **not** verify an individual application RSS guard. Missing, malformed, or insufficient captures remain explicit gaps, never zero-cost findings. Prior failed windows and the unexecuted aggregate wrapper remain historical evidence.
