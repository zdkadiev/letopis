# November 17, 2026 performance study

Start with the [final report](../../reports/Letopis%20comprehensive%20performance%20profiling.md) and the
[source-linked knowledge](../../letopis/development/performance-bottlenecks.md#profiling-method-2026-11-17).

This documentation bundle publishes the report, research notes, audits and original
profiling specification/plan. It does **not** publish the approximately 3.4 GB local
evidence tree or the one-off research tools. Production code and dependencies are
unchanged. Optimization work is tracked separately:

- [#111: status/check source verification](https://github.com/zdkadiev/letopis/issues/111).
- [#112: full-index graph publication](https://github.com/zdkadiev/letopis/issues/112).

## Reading order

| Subject | Completed results | Independent audit |
|---|---|---|
| 60 / 1k / 10k baselines | [Canonical survey](canonical_survey.md) | [Canonical evidence audit](canonical_evidence_audit.md) |
| 100k baselines | [Canonical stress](canonical_stress.md) | [Stress audit](canonical_stress_audit.md) |
| Main CPU profiles | [Canonical CPU profiles](canonical_cpu_profiles.md) | [CPU audit](canonical_cpu_audit.md) |
| 159 supplementary cases | [Supplementary results](supplementary_results.md) | [Catalog audit](supplementary_evidence_audit.md) |
| Remaining command CPU profiles | [Direct-root results](direct_root_cpu_results.md) | [Direct-root audit](direct_root_cpu_audit.md) |
| Four allocation-prefix snapshots | [Memory results](memory_results.md) | [Memory audit](memory_evidence_audit.md) |
| 2 / 100 / 1,000-text mock drains | [Drain scale results](drain_scale_results.md) | [Drain audit](drain_scale_audit.md) |
| Report readiness | [Writing check](report_writing_check.md) | [Final recheck](report_final_recheck.md) |

Read [build validation](build_validation.md), [methodology](profiling_methodology.md)
and [workload design](workload_design.md) for provenance and measurement boundaries.
Earlier review failures and unstarted preflights remain historical records, not
the current completion verdict. The final report and `report_final_recheck.md`
supersede the earlier `report_final_review.md` verdict. Likewise, the completed
drain results at the top of `drain_scale_results.md` supersede its earlier
unstarted-attempt records.

## Evidence availability

References containing `evidence/` or `tools/` are **local archive references**.
They deliberately retain the paths recorded during the study and are not
downloadable artifacts in this repository. This includes raw JSON/JSONL rows,
Samply profiles and symbol sidecars, native heap graphs, logs, executables,
build receipts, copied fixtures, and one-off launcher scripts. macOS `file://`
manual references also describe the original host rather than portable web links.

The Markdown results and audits contain the measurements, sample counts, method,
source locations and qualifications needed to understand the conclusions. They
do not replace raw evidence for independently recomputing the study. Obtain the
retained local archive, or publish a separately reviewed subset, before attempting
raw-profile verification or exact archived-run reproduction. Do not treat an
unavailable raw artifact as a passed check.

Archived absolute paths are host-specific. Use a fresh output directory and
genuine matching build receipts for any new run; never execute an old launcher
unchanged against its original outputs. The original plan's prohibition on
staging/committing concerned the measurement phase; later publication of these
documents was explicitly requested.

The knowledge's new `source_snapshot` points to the complete tree of the report
commit preceding the knowledge commit in this PR. Its declared source bytes were
compared with that tree. This makes the new source basis reachable from branch
history without publishing raw captures or depending on a local-only retention ref.
