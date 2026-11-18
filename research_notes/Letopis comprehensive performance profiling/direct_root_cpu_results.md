# Direct-root CPU results

**Eight categories attempted; 15 finite captures: six usable and nine insufficient.** Eight 1 kHz attempts plus seven one-time 10 kHz diagnostics; no errors or unstarted categories. All sampler processes exited 0 and requested stdout counts validated. Per-child exit codes are not independently observed; sampler completion is not a per-child exit trace. No censored sampler invocation observed. This is not baseline latency.

| Case | Rate Hz | Product stacks | Sampled PIDs | Validated outputs | Outcome |
|---|---:|---:|---:|---:|---|
| `sparse-100.init-fresh` | 1000 | 43 | 1 | 1 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-1000/attribution.json)) |
| `sparse-100.init-fresh` | 10000 | 277 | 1 | 1 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.init-fresh-10000/attribution.json)) |
| `sparse-100.config` | 1000 | 64 | 64 | 64 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-1000/attribution.json)) |
| `sparse-100.config` | 10000 | 664 | 61 | 64 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.config-10000/attribution.json)) |
| `sparse-100.coverage---help` | 1000 | 7 | 5 | 64 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-1000/attribution.json)) |
| `sparse-100.coverage---help` | 10000 | 38 | 16 | 64 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---help-10000/attribution.json)) |
| `sparse-100.coverage---version` | 1000 | 0 | 0 | 64 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-1000/attribution.json)) |
| `sparse-100.coverage---version` | 10000 | 8 | 3 | 64 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.coverage---version-10000/attribution.json)) |
| `sparse-100.mock-drain` | 1000 | 638 | 1 | 1 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-drain-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-drain-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-drain-1000/attribution.json)) |
| `sparse-100.mock-published` | 1000 | 27 | 1 | 1 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-1000/attribution.json)) |
| `sparse-100.mock-published` | 10000 | 288 | 1 | 1 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-100.mock-published-10000/attribution.json)) |
| `sparse-10000.index-verify` | 1000 | 54 | 1 | 1 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-1000/attribution.json)) |
| `sparse-10000.index-verify` | 10000 | 734 | 1 | 1 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/sparse-10000.index-verify-10000/attribution.json)) |
| `long-1000.traverse` | 1000 | 49 | 1 | 1 | insufficient ([raw](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-1000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-1000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-1000/attribution.json)) |
| `long-1000.traverse` | 10000 | 529 | 1 | 1 | usable ([raw](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-10000/profile.json.gz), [sidecar](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-10000/profile.json.syms.json), [attribution](evidence/direct-root-cpu-20261117-v1/attempts/long-1000.traverse-10000/attribution.json)) |

## Selected inclusive product-library scopes

Numbers are inclusive weighted sampled CPU in microseconds. Denominator is observed all-library CPU for matching-library-derived PIDs, including native frames; function weights overlap and must not be summed. They are not wall-time percentages, causal proof, or independent PID traces. Only usable captures shown.

| Case (rate) | All-library observed CPU µs | Example product-library scope | Inclusive µs |
|---|---:|---|---:|
| `sparse-100.init-fresh` (10000) | 30227 | `letopis::git::repository::GitRepository::open_with_cleanup` | 15106 |
| `sparse-100.config` (10000) | 473365 | `letopis::git::repository::GitRepository::open_with_cleanup` | 390922 |
| `sparse-100.mock-drain` (1000) | 108189 | `letopis::service::embedding::EmbeddingService<C,V,F,M,P>::drain_ready` | 93614 |
| `sparse-100.mock-published` (10000) | 39704 | `letopis::wire_embedding_executor_at_root` | 23660 |
| `sparse-10000.index-verify` (10000) | 68848 | `letopis::service::document_refresher::DocumentRefreshService<G,F,M,P,C>::detect_changes` | 42209 |
| `long-1000.traverse` (10000) | 53442 | `letopis::service::traversal::LinkObservations<F,C>::get_knowledge_link_by_element` | 23286 |

Help and version remained insufficient after their single 10 kHz diagnostic; do not infer zero CPU or hotspots. Config/help/version requested and validated 64 outputs with EOF. Matching PIDs may be fewer than 64 because not all processes yielded product-library samples; per-child exits remain unobserved.

## Semantic and identity checks

- Busy drain: 100 distinct hashes, queue 100→0, zero failures, 100 loopback requests and 100 items; native current-config observer confirms 100 covered, zero missing, 32 dimensions.
- Populated hybrid query: 100 vectors before query, one local mock request and no query errors; native observer confirms 100 covered at 32 dimensions before and after against the same disposable database and configuration.
- Sparse 10k verified index retained 10,000 nodes/edges with unchanged revision. Long-text 1k traversal returned 16 elements and 16 edges at depth 8; it did not return 1,000 nodes.
- Each attempt verified the 116-input full build receipt, a private executable and adjacent dSYM against build metadata and UUID. Profiles with product frames matched the exact private executable path and UUID; target PIDs derive from those frames, not an OS exec trace. [Independent post-run hashes and sample inventory](evidence/direct-root-cpu-20261117-v1/verification.json).
- Local numeric-loopback deterministic mock only; implicit workers suppressed by owned lock. No real model/provider measurement. Samply requested 1 kHz or one explicitly labeled 10 kHz diagnostic; profiles are separate from stock baselines. No same-build ordinary controls were captured.

## Procedure and resource limits

[Prepared fixture manifest](evidence/direct-root-cpu-20261117-v1/manifest.json), [one-off orchestrator](evidence/direct-root-cpu-20261117-v1/run.py), [campaign records](evidence/direct-root-cpu-20261117-v1/campaign-results.json), and per-attempt capture, stdout/stderr, validation, native and attribution receipts are preserved. Exact Samply argv is in each `capture.json`.

```sh
/opt/homebrew/bin/python3.12 -B "research_notes/Letopis comprehensive performance profiling/tools/profile.py" prepare --evidence "/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/direct-root-cpu-20261117-v1" --sizes 100,10000
/opt/homebrew/bin/python3.12 -B "research_notes/Letopis comprehensive performance profiling/evidence/direct-root-cpu-20261117-v1/run.py"
```

Reproduction requires new evidence and updated absolute paths; never overwrite this run. Shared stage completed within 900 seconds; per-attempt limit 120 seconds. Final scratch plus evidence was 1,026,221,086 bytes with 56,527,007,744 bytes free. The 4 GiB/free-20-GiB checks were soft observations, not live limits. Sampler RSS cannot verify the product RSS guard. Old catalog JSONL and final-audit hashes unchanged; no build, heap or 100k stress run.

The first one-off orchestrator launch stopped on a Python syntax error before it started any case or process. The orchestrator was corrected and launched once; no sampled attempt was repeated beyond the explicitly allowed 10 kHz diagnostics.
