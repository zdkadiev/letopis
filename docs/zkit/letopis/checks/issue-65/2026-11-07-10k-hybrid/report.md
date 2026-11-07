# 10k hybrid search comparison

2026-11-07. Synthetic corpus: 10,000 knowledge, 10,000 source nodes, 50,000 edges, 334 Markdown files; 10,000 real 768-dimensional Ollama embeddinggemma vectors. 9,999 vectors were copied by matching search hash from the previously measured 100k corpus; the remaining vector was generated through Ollama. Query `case 000000`; fresh CLI processes; model warmed and OS cache uncontrolled.

| Limit | Before median | After median | Speedup |
| --- | ---: | ---: | ---: |
| 5 | 5.293793 s | 0.181590 s | 29.15x |
| 100 | 5.260998 s | 0.291299 s | 18.06x |

Three alternating pairs per limit; independent SQL oracles validated IDs and each result had empty flags. All results were byte-identical within each limit. No provider failures or query errors and full vector coverage after measurement. CLI wall time includes query inference, not corpus preparation. This is one query on a synthetic fixture, not a production or general relevance claim. Earlier 100k campaign is separate; cache state and host load were not controlled.

[Summary](summary.json), [run log](bench.log), [measurement driver](bench.py), and [SQL oracles](oracle/). Individual measurement receipts accompany them.
