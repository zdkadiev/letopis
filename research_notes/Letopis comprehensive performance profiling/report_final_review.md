# Independent final report review

**Verdict: NOT READY TO DELIVER.** One important presentation gap prevents the report itself from satisfying the requested per-command CPU/memory/timing comparison. No new measurements are needed.

## Critical

None found.

## Important

1. **Representative per-command results omit measured CPU and largely omit load comparisons.** `reports/Letopis comprehensive performance profiling.md:36–46` gives wall time, n, RSS and sampled attribution, but no stock CPU seconds for any row. The separate timing table at lines 55–65 supplies CPU for full index, 10k check/status/search/traverse and first-publication search, but not init, config, inspect, or their representative scenarios; the drain table supplies separate CPU. Consequently readers cannot compare wall, CPU and RSS for every public command from the report, and the 0/60/100/1k/10k/100k workloads are not presented as a meaningful per-command comparison. A sampler percentage is not a substitute for baseline CPU seconds.

   **Fix:** Add CPU seconds to the command table for the exact same scenario and n, using the saved rows or qualified summaries. For canonical 100k the independent stress summary records CPU medians: status 10.161579 s, check 10.464636 s, inspect 5.444781 s, search 0.680672 s, traverse 0.655148 s, no-op index 0.655125 s, full index 174.625770 s (`canonical_stress.md:47–56`). For init/config use the exact same sparse cohorts from `supplementary_evidence_audit.md:16–20`; report user and system medians separately, not their sum as if it were the median of per-run totals. Drain already has user/system medians at report lines 71–75. Add a compact, identity-labeled selection of existing small and large workload rows or an explicit per-command table linking to exact load-specific rows with n, wall, CPU and RSS. Preserve canonical versus sparse identities; do not imply every command ran at every scale or demand new probes.

## Minor

1. **Review state will be stale at delivery.** Report line 110 says “Independent report review remains pending.” Replace after review disposition with the actual final state, or remove that sentence. Do not imply approval until the important finding is resolved and checked.
2. **Reproduction prerequisites are hard to locate.** Lines 122–132 correctly require new output paths and warn about fixed paths, but the supplementary link points first to results rather than explicitly identifying the required full product-build receipt/metadata and native observer. Add a direct link to `supplementary_preparation.md:5–15` and the archived build/metadata in `build_validation.md:11–24`; explain that supplementary mock/vector reproduction needs the matching native observer and full receipt, and that archived host-specific executable paths and launchers must be adapted, not assumed portable. Do not rerun into existing evidence.

## Independent checks and boundaries

- Read the frozen report, design context, `report_writing_check.md`, canonical survey/stress/CPU and supplementary/memory/direct-root audits, relevant saved summaries and final verification logs. No product, profiler, build, test, benchmark or measurement probe was executed; raw evidence and report were not modified.
- `canonical_stress-100000-20261117-v1/resource_summary.json:14–49` independently confirms disabled OpenAI fixture, 100,000 knowledge, 100,000 source, 500,000 edges, 73,333 includes, 2,000-byte fragments, 19 measured plus one first use and 465,158,144-byte highest recorded RSS. Stress audit lines 15–26 confirms full index 178.498726084 s and ordered inspect 47.384810125 → 7.147968166 → 7.878149083 s; n=1 and cache limitations are retained.
- Supplementary audit lines 7–10 confirms 159 validation, 740 baseline and one expected exit-1 baseline; together with canonical 319, stress 19 and drain 9 gives 1,087, without mixing fixture identities. The 45-node, 204-edge real projection, unavailable source-node count and 89 findings are accurately qualified at report lines 27–30.
- CPU audit lines 7–13 and direct-root audit lines 3–14 support 12 canonical captures, 15 direct captures (six usable, nine insufficient), separate unattributable windows and qualified sampled percentages. Current `src/service/document_refresher.rs:381–392,949,983–989` supports the described repeated scans; O(FE)/O(SN) remain analytic hypotheses, not measured counts or promised savings. Report distinguishes sampled inclusive CPU from elapsed time and non-additive nested scopes.
- Memory audit lines 19–27 supports four prefix snapshots, not full-lifetime allocator peaks or leak proof. RSS scope, VM reservation and non-stock instrumented values are properly distinguished. Local mock is 32-dimensional and not a real provider/model; no independent network trace is claimed.
- Final logs report 94 tests in 21.287 s, eight skipped, and 49 tests in 2.372 s, both OK. Historical earlier failure is not reported as a current failure or proven underlying fix.
- Read-only local link scan found 49 Markdown links, zero missing filesystem targets after URL decoding; this is not an anchor or semantic-content check. Current working tree shows only untracked research/spec/report artifacts, with no tracked production diff; no claim about unobserved historical workspace state follows.
