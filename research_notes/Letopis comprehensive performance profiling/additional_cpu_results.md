# Additional command CPU capture

Four debug1 validations passed. Four sampled child processes exited 0, but **0/4 CPU profiles are attributable to the product**: each failed the frozen driver’s product-library UUID check. No profile is usable for weighted CPU/source conclusions; 0 matching product-PID stacks observed in each raw profile is insufficient, not zero product CPU. No retry or ordinary control was run.

| Case | Validation | Sampled child exit | Product-PID stacks observed | Result |
|---|---|---:|---:|---|
| `sparse-100.mock-drain` | PASS | 0 | 0 | INSUFFICIENT / identity unavailable ([raw profile](evidence/additional-command-cpu-20261117-v1/receipts/51bcc4d8798041358b6acdd682c08f51-sparse-100.mock-drain-0/window/profile.json.gz)) |
| `sparse-100.mock-published` | PASS | 0 | 0 | INSUFFICIENT / identity unavailable ([raw profile](evidence/additional-command-cpu-20261117-v1/receipts/ca2d375b6135463288ff74ba9577c466-sparse-100.mock-published-0/window/profile.json.gz)) |
| `sparse-10000.index-verify` | PASS | 0 | 0 | INSUFFICIENT / identity unavailable ([raw profile](evidence/additional-command-cpu-20261117-v1/receipts/cf1f7faa5c4e4674984189cb68b97b43-sparse-10000.index-verify-0/window/profile.json.gz)) |
| `long-1000.traverse` | PASS | 0 | 0 | INSUFFICIENT / identity unavailable ([raw profile](evidence/additional-command-cpu-20261117-v1/receipts/f37df499968a4c9e9efb639531728228-long-1000.traverse-0/window/profile.json.gz)) |

## Observations and limits

- Product executable and dSYM matched the debug1 build receipt before launch. Raw captures contain sampler/Python thread records but no thread whose PID equals the actual child PID, and no `letopis` library entry. Thus captured profile/sidecar cannot prove matching runtime code ID; cause not established. The driver rejects before weighted attribution. No CPU top functions or source locations can be reported.
- Validation drain: 100 queued before, 0 after, 100 mock requests/items; native observer reports 100 covered, 0 missing, 32 dimensions. Enabled published query: 100 prepopulated vectors, one mock request, zero query errors. Disabled 10k `index --verify`: 10,000 nodes and edges stable, revision unchanged and no provider requests.
- Long-text traversal validated against the driver oracle: depth 8, both directions, requested limit 100; actual result 16 elements and 16 edges, not 1,000 returned nodes.
- Each profile used existing samply owner, requested 1,000 Hz and at most a 10-second window plus 30 seconds sampler finalization; child exit and stdout/stderr are preserved in each `window/child.json`. A matching binary/dSYM UUID preflight does not prove profile identity. Sampler wrapper elapsed time is not baseline latency. No profiler overhead claim.
- [Raw JSONL](evidence/additional-command-cpu-20261117-v1/measurements.jsonl), [manifest](evidence/additional-command-cpu-20261117-v1/manifest.json), [exact per-case commands, outputs and phase state](evidence/additional-command-cpu-20261117-v1/control/), and sidecars adjacent to each linked raw profile. Stage phase budget 600 seconds, command timeout 120 seconds; once per case. Each profile was attempted once after its matching debug1 validation.
- Preparation invocation: `/opt/homebrew/bin/python3.12 -B "research_notes/Letopis comprehensive performance profiling/tools/profile.py" prepare --evidence "/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/additional-command-cpu-20261117-v1" --sizes 100,10000`. Prepared input fixtures and driver hash verified after captures.
- No controls, profiles outside these four, builds, native heap, 100k workload, product changes or commits. Local mock only. Existing catalog JSONL and final-audit SHA-256 match the values recorded before this phase.
