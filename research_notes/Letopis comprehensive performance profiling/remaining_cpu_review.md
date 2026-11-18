# Four-command CPU launcher review

**Verdict: BLOCK for bounded pilot** pending one attribution correction below. Static review only: no tests, launcher CLI, product, sampler, tool probes, or active measurement progress reads.

## Blocker

- [`launcher.py:171–176, 189–212`](evidence/remaining-cpu-prepared-v1/launcher.py#L171-L212): the matching-product-library `product` counter is used only as a positive-CPU gate and then discarded. The saved `functions_inclusive_cpu_delta_us` instead comes from `summarize_stacks`, which includes every resolved or fallback function in target-PID stacks, without a product-library restriction. A consumer cannot distinguish product-library attribution from system/other-library names in that output. **Save the already-computed product-library-weighted counter separately**, with its PID and inclusive-overlap scope; retain the broader counter only if explicitly labeled as all-library target-PID stacks. Preserve the positive `letopis::` weight gate. This is required for the requested product-library-weighted attribution, not a request for new command integrations.

## Other reviewed boundaries

- Fresh Git repository per `init` child; config setup uses a separate initialized disposable repository; help/version send EOF. Child outputs, exit codes, hashes and unique PIDs are recorded, with init database and config invariance checks. Child and sampler failures invalidate the aggregate.
- Source/build receipt, expected binary hash and UUID, copied executable and dSYM hashes are checked. Profile and symbol sidecar identities are checked against the private binary; target child PIDs restrict CPU samples. Zero product CPU or no sampled PID fails closed, not zero-cost evidence. A positive but sub-threshold five-child pilot is explicitly `insufficient` and may proceed; a zero-sample pilot aborts rather than proving no cost.
- Four requested command aggregates only; 1 kHz first, at most one separately labeled 10 kHz diagnostic for each insufficient capture. No drain, query or traversal integration required for this scope.
- Existing `measure_process` owns the sampler process group; wrapper kills and reaps its active child on timeout/interruption. Commands have individual timeouts and the sampled aggregate has a 120-second cap. Fixture preparation occurs before the aggregate timer and checks its own subprocess timeouts; the 900-second phase deadline is checked between captures, not a hard end-to-end deadline including all preparation.
- External execution-owner checks must confirm combined evidence below 4 GiB and free disk at least 20 GiB before work and inspect receipts after captures. These are soft controls under the coordinator ruling. Aggregate wrapper RSS is not a substitute for individual CLI RSS: **no per-app profile-only RSS guard is verified**. Prior stock baselines are separate evidence, not this aggregate.

Static review does not establish sampler format compatibility, actual short-process visibility, runtime cleanup, or successful child oracles. Pilot output and raw receipts must establish those facts after ownership transfer.
