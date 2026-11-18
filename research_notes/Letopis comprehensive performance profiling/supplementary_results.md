# Supplementary campaign results

**159/159 cases validated; 159/159 baseline series completed.** 159 validation rows and 740 baseline rows; no skips, failures, missing slots or duplicate sample IDs. All 1,798 foreground stdout/stderr files verified against recorded hashes. Raw setup receipts are separate. These are supplementary fixture results, not canonical workload replacement.

## Predeclared repetition policy

| Case class | Cases | Measured slots |
|---|---:|---:|
| n=1 | 13 | 13 |
| n=3 | 129 | 387 |
| n=20 | 17 | 340 |

Coverage-only aliases/help/version/invalid argv use n=1; ordinary cases n=3; non-help init/config on sparse 0/100/1,000/10,000 n=20. Explicit `--repeats` means **no driver-added first-observed or conditioning slots**; these are not claimed. The prior canonical campaign retained its original defaults. No p95/p99 reported.

## Selected series

| Case | n | Median wall s | Min–max wall s |
|---|---:|---:|---:|
| `sparse-0.init-fresh` | 20 | 0.066172 | 0.058114–0.170055 |
| `sparse-100.config` | 20 | 0.016592 | 0.011428–0.067299 |
| `sparse-100.index-noop` | 3 | 0.028324 | 0.027805–0.029022 |
| `sparse-100.inspect-batch` | 3 | 0.101851 | 0.097977–0.104822 |
| `sparse-100.mock-drain` | 3 | 0.994815 | 0.960566–1.066349 |
| `canary-2.vector-only` | 3 | 0.034235 | 0.031659–0.076338 |
| `sparse-1000.search-selective` | 3 | 0.034531 | 0.031064–0.034707 |
| `sparse-10000.index-full` | 3 | 1.165611 | 1.126559–1.180543 |
| `real.index-full` | 3 | 0.080278 | 0.078327–0.085856 |

## Coverage and procedure

- Full 159-case outcome and per-series identity/median/min/max/sample standard deviation: [case-results.json](evidence/supplementary-prepare-20261117-v1/campaign-control/case-results.json). Catalog includes all eight public commands, internal drain, global/command help, version, combined flags, invalid argv, sparse scales, graph/source/long-text variants and bounded own-repository content snapshot.
- [Raw rows](evidence/supplementary-prepare-20261117-v1/measurements.jsonl), [case inventory](evidence/supplementary-prepare-20261117-v1/case-inventory.md), [final audit](evidence/supplementary-prepare-20261117-v1/campaign-control/final-audit.json), and [batch argv, logs and audits](evidence/supplementary-prepare-20261117-v1/campaign-control/). Each sample preserves campaign/build/config/fixture/state identity, exact request, stdout/stderr/exit, wait4 raw user/system CPU, RSS, faults and block-operation counters, and separate setup receipts.
- All 144 remaining cases validated once after 15 previously validated cases; only exact passing identities entered baseline. All batches ran serially, validations capped at 900 s and baselines at 1,800 s aggregate; per-call 120 s. One-off [orchestration](evidence/supplementary-prepare-20261117-v1/campaign-control.py) records requested counts before collection. Final audit has no unobserved, missing, unexpected or duplicate slots. All immutable fixture hashes, harness and infrastructure hashes reverified after collection.
- Mock endpoint was numeric loopback only. Implicit detached drain was suppressed by owned lock; explicit drain was measured separately. Mock protocol is not real model/provider performance. OS caches uncontrolled; per-child wait4 can include OS-accounted reaped descendants, peak RSS is lifetime peak, not simultaneous tree peak or allocation profile. OS block operations are not byte I/O. Setup is outside the reported foreground wall interval; raw setup resource observations remain separate.
- Soft combined scratch/evidence guard was stricter 2 GiB in frozen driver; additional outer 4 GiB requirement therefore never approached. Final observed combined logical bytes 850,210,493 with 59,642,589,184 bytes free. Soft observations are not live hard protection; post-sample 1 GiB RSS stop was not reached.

## Canary limitation

An independent read-only query of the original canary cache FTS index is **unavailable**: the driver removes the disposable sample repository, and no cache.db remains in its scratch. No product rerun was performed. Existing canary receipt shows the expected ID and one local request; the query token is absent from frozen input text, but this is not a direct FTS MATCH result. This gap is not counted as a successful FTS query.

No profiling, heap capture, new 100k workload, real provider, additional embedding dimensions or production changes.
