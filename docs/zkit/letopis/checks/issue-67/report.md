# Product Git optimization (#67): measurement report

Task 1 is complete: the comparison harness and fresh pre-optimization baseline
exist. Product optimization, compatibility review, optimized measurements and
final acceptance are pending. The old #65 measurements remain historical.

The source revision captured for the baseline is
`881cc6417073110e852fbebf2b9bce5ed1ea3d29`; all 156 product inputs match
`32ca0af282a1321d0137b4aba115c6fddc086259`. The executable SHA-256 is
`a132acb0a19c24440e8e97b881657c7b7adcc5c17052469f5dbc9d1e1b336f62`.
The release uses `rustc 1.95.0 (59807616e 2026-04-14)`, locked offline Cargo
inputs and `CARGO_PROFILE_RELEASE_DEBUG=1`. Frozen sources, executable, available
symbols and the original release receipt are indexed in [evidence-index.md](evidence-index.md).

The baseline ran on an Apple M4 Pro, arm64 macOS 26.6.2, with 48 GiB physical
RAM. Fixture settings are seed 65, 60 elements, 2,000 bytes per fragment, 30
elements per file and disabled embeddings. OS page cache and host load are
uncontrolled. Each timed call is a fresh CLI process; initialization and fixture
copy refresh are outside initialized-check latency.

| Scenario | Before median / n | After median / n | Reduction / speedup | Git subprocesses before → after | Peak memory before → after / scope | Correctness / limits |
| --- | --- | --- | --- | --- | --- | --- |
| Initialized check, 60 elements with external links | 13.612073833 s / 10 | pending | pending | not assessed → pending | wait4 receipts available; comparison pending | 10/10 valid; min 13.497470333 s, max 14.012865666 s; 2 warmups excluded |
| Initialized inspect, 1k | pending | pending | pending | pending | pending | pending |
| Bounded traverse, 1k | pending | pending | pending | pending | pending | pending |
| FTS search, 1k (control) | pending | pending | pending | pending | pending | pending |
| First publication and full audit, 1k | pending | pending | pending | pending | pending | 600 s initial / 120 s ordinary budget planned |
| Reindex with concurrent reader, 60 | pending | pending | pending | pending | pending | Kernel-lock acknowledgement and actual overlap required |

First-use search at 60 took 14.308402375 s once and passed its independent
oracle. It is separate from initialized-check timings. The baseline had no
timeouts, invalid outputs or stage-budget exhaustion. No p95/p99 is claimed
from ten samples. The provisional 50% reduction goal awaits an optimized build.

`make check` passed before freezing the baseline. The final Task 1 Python
performance suite passed 72 tests with eight explicit missing-pin integration
skips. The first sandboxed suite had two process-inspection permission errors;
the permitted rerun passed. No real-provider sessions, profiles, contention
campaigns, larger scales or Linux/native runner were executed in Task 1. This
report does not establish final product acceptance or whole-stack memory usage.

Harness stages, schedules, receipt validation and scope limits are documented
in [the performance README](../../../../../tests/performance/README.md#frozen-optimization-comparisons-67).
The existing single-revision build verifier remains checkout-bound. The new
frozen verifier checks archived sources and artifacts independently of later
checkout changes; local receipts are provenance records, not signed compiler
attestations.
