# Canonical 100k stress evidence audit

2026-11-17. **PASS with provenance and measurement qualifications.** Read-only analysis of completed 100k artifacts; no product/build/benchmark/test execution, sampler-output inspection, or changes to raw evidence.

## Findings

- **Qualification — provider wording:** `manifest.json` says `no API calls`, and raw rows label `embedding_mode` as `disabled_openai_no_provider_calls`. These are configuration/intent assertions, not network observations. The retained configuration has embeddings disabled, while `canonical_stress.md:35-41` correctly states that traffic was not independently traced.
- **Qualification — single observation:** full index ran **once**, 178.498726084 s. The report puts this value in its median column (`canonical_stress.md:56`), which is mathematically the median of one sample, but it is **not a replicated series estimate**. Its explicitly stated n=1, preregistered reduction and lack of confidence/tail claims are correct. First publication is a separate single observation, 135.650654625 s.
- No numerical discrepancy found in the requested raw timing, phase, count, RSS or guard checks.

## Raw sample verification

Raw `samples.jsonl` SHA-256 is `7a1eea909bd64e11550d7a379e7399527605b6643e978aee8b66243526b22e3e`, matching `canonical_stress.md:81`. All **20/20** recorded rows have `outcome=success`, exit 0 and `validation.valid=true`: one first-use search and 19 measured calls. Six measured scenarios have three rows each; `index --full` has one. Three no-op `index` responses each say `published:false`. All captured, untruncated stdout hashes match their bytes. Recorded argv confirms the commanded operations; it is not independent operating-system exec tracing.

| Case | n | Raw wall median or single observation, seconds | RSS median / max, bytes |
|---|---:|---:|---:|
| search, first publication | 1 | 135.650654625 | 381,337,600 / 381,337,600 |
| check | 3 | 10.859230250 | 389,431,296 / 391,348,224 |
| status | 3 | 10.679150708 | 387,530,752 / 391,905,280 |
| inspect | 3 | 7.878149083 | 62,996,480 / 63,504,384 |
| search | 3 | 0.727827334 | 18,743,296 / 19,660,800 |
| traverse | 3 | 0.682389834 | 18,710,528 / 18,972,672 |
| no-op index | 3 | 0.680731208 | 18,006,016 / 18,120,704 |
| full index | 1 | 178.498726084 | 465,158,144 / 465,158,144 |

Inspect wall times in execution order are 47.384810125, 7.147968166 and 7.878149083 s; their median must not hide that ordering. Full index is the highest individual recorded lifetime RSS, **465,158,144 bytes = 443.609375 MiB**, not a combined instantaneous process-family peak or allocated heap. All rows label RSS bytes and CPU seconds; wait4 scope includes OS-accounted reaped descendants.

## Corpus, executable, guard

- Corpus and independent-validation receipts agree on seed 65, 100,000 knowledge elements, 100,000 source nodes, 500,000 edges, 73,333 includes, 2,000-byte fragments and 30 elements per file. These semantic counts are validator receipts, not a second validator execution during this audit.
- Corpus manifest contains exactly **4,334 input hashes**: **3,334 knowledge files and 1,000 source files**. All 4,334 current retained input files match their recorded SHA-256; no hash drift. The fixture manifest and disabled OpenAI config match the hashes in `corpus_validation.json`.
- Build receipt identifies revision `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. The actual archived executable SHA-256 equals the pinned `77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`; all 116 pinned product-input hashes match current files. All 27 archived harness file hashes and the captured one-off launcher hash match `manifest.json`. These checks validate file identity, not a compiler attestation or an independent trace of each invocation.
- The **42** retained disk observations start at **803,778,560 bytes** and reach a maximum/final **1,169,702,912 bytes ≈ 1.09 GiB**, below the 2 GiB soft guard at observation points. This is allocated total evidence space, not memory, bytes written, or a continuously observed disk peak. The 1 GiB process-RSS rule is checked **after** a call; neither rule is a physical hard watchdog.
- One shared fixture and uncontrolled OS cache preclude cold-start or independent-replication claims. No provider throughput, sampled hotspot, confidence interval, p95/p99, or production capacity inference follows from this bounded run.
