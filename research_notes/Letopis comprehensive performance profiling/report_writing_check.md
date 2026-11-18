# Report writing check

Report: `reports/Letopis comprehensive performance profiling.md`. Checked after synthesis against the completed results and qualified audits, including `direct_root_cpu_audit.md` and coordinator's final verification handoff.

- Word count after review fixes: 3,915, using Python regular expression `\b[\w]+(?:[-'][\w]+)*\b`.
- Relative Markdown links after review fixes: 57 checked for filesystem existence after URL decoding; zero missing. This checks paths, not fragment anchors or the contents of external files.
- Canonical counts: 352 = 319 measured + 30 conditioning + 3 first use; 100k 20 = 19 measured + 1 first publication. Catalog 159 validations + 740 baseline, drain 3 validations + 9 baseline; baseline 319 + 19 + 740 + 9 = 1,087, including one expected invalid-argv exit 1.
- Catalog 899 total rows and 1,798 stream hashes; baseline repetition 13×1 + 129×3 + 17×20 = 740. Drain's distinct 100-text cohort is not pooled with the catalog.
- Full 100k stock index 178.498726 seconds, 443.61 MiB, n=1; sampled complete run 185.133 seconds wrapper, 177.888334 seconds observed CPU, 183,971 stacks. Inclusive weights 53.35%, 23.92%, 22.63% overlap.
- CPU capture counts: 12 validated canonical; direct-root 15 captures, six usable and nine insufficient; help/version insufficient; four earlier windows unattributable. Four live memory snapshots are not full-lifetime allocation profiles.
- Fresh final helper verification: 94 tests, eight skips, OK; research tools 49 tests, OK. Earlier suite error remains historical; no inference that its underlying timeout cause was fixed. Report review remains pending.
- Local link/count check executed with Python read-only inspection. No product, profiler, build or measurement probe was run in report synthesis.

## Scoped review fixes and CPU derivation

Review `report_final_review.md` requested representative baseline CPU, an eight-command small/large table and direct reproduction prerequisites. Only those report sections and the obsolete pending-review sentence changed; no measured evidence or product files changed. Startup rows filter `measurements.jsonl` by exact `case_id` and `kind=baseline`; drain filters the separate `drain-scale-20261117-run-v2/measurements.jsonl` identically. For each row compute `user_seconds + system_seconds`, then take the median; do not add marginal medians. Independent coordinator recomputation agrees:

| Exact cohort | n | Median combined CPU seconds |
|---|---:|---:|
| `sparse-0.init-fresh` | 20 | 0.015274500 |
| `sparse-10000.init-fresh` | 20 | 0.015353000 |
| `sparse-100.config` | 20 | 0.004777500 |
| `sparse-10000.config` | 20 | 0.004917000 |
| Separate drain sweep `sparse-100.mock-drain` | 3 | 0.175816000 |

The drain per-run combined CPU values were 0.178066, 0.175816 and 0.153645 seconds. Canonical small and large CPU values come from the saved survey and stress summaries, with their own cohort identities and n. The new comparison uses canonical 60/100k for the six source-rich commands, and sparse startup cohorts only for init/config. Existing drain-table user and system medians are explicitly separate marginal summaries, not combined CPU.

Targeted review of the presentation fixes remains pending. No additional experiment, build, product call or test was run.
