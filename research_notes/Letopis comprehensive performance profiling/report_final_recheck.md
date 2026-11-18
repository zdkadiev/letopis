# Targeted final report recheck

**READY TO DELIVER.** The prior important finding and both minor findings are resolved. No new blocking issue found in the requested scope.

- Report lines 34–46 now provide same-cohort n, wall, combined CPU and RSS for all eight public commands and the separate 100-text drain; sampled attribution remains distinct. The config profile's 64 outputs and 61 sampled PIDs are not presented as its baseline n=20.
- Lines 51–64 compare each public command across explicitly labeled small and large cohorts, without pooling canonical and sparse fixtures or claiming a controlled scaling experiment. Fresh `init` is correctly a startup cohort, not an indexed-graph workload.
- Independently recomputed combined CPU as median of per-run user plus system from saved baseline rows: sparse-0 init 0.0152745 s, sparse-10k init 0.015353 s, sparse-100 config 0.0047775 s, sparse-10k config 0.004917 s, and separate 100-text drain 0.175816 s. Sample counts were 20, 20, 20, 20 and 3 respectively; saved wall and RSS medians agree with the new rows. Canonical 100k figures match `canonical_stress.md:47–56`.
- The frozen report no longer says independent review is pending. Its reproduction section at lines 137–149 links stock and symbolized receipts, build metadata, native observer receipt and binary, and supplementary preparation; it requires new evidence paths and warns that archived paths are host-specific.
- Independently checked 57 local Markdown link targets after URL decoding: zero missing. This checks filesystem existence, not anchors or contents.

This was a targeted read-only recheck, not a new full evidence audit. No product, build, test, benchmark, profiler or measurement probe ran; the frozen report and raw evidence were not edited. The earlier `report_writing_check.md` statements that review was pending describe its preparation-time state; this recheck supplies the later disposition.
