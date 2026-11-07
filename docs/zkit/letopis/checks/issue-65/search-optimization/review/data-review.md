# Independent final measurement audit

2026-11-06. Data-only audit; no product/source edits, builds, test suites, provider
calls, native-vector query reruns, or benchmark calls. Only this report was written.

## Verdict

No Critical or Important finding. No material missing artifact or arithmetic
discrepancy found. The matched result is supported: hybrid median
111.88586795795709 -> 61.10203754086979 seconds, 45.38895871654688% reduction,
1.8311315376859427 times speedup, with three samples per side and six valid calls.
This is evidence for the measured synthetic corpus with real-provider vectors,
on the recorded shared host, without a production/minimum-hardware verdict.

One Minor report wording correction was communicated to the coordinator: replace
the opening "Full100k real hybrid search" with explicit synthetic-corpus and
real-provider wording. Its fixture description and numerical claims are otherwise
consistent with the artifacts. This is a reporting clarification, not a data or
product defect.

## Ordinary calls and calculations

Independently decoded and hashed every raw/final stdout/stderr capture from all
six pairs records and ten FTS records. Raw records equal final records after
removing the added validation field. Every call has exit 0, success outcome,
untruncated captures, empty stderr, ok:true, no warnings, and clean returned flags.
Checked each per-call request, source-input manifest, binary identity, fixture
counts/configuration, vector coverage, and literal result cardinality.

All six hybrid outputs are byte-identical, SHA-256
`e5b1d05b7485793074092687c9c1d5cae6e8c178f267199c2eac309f9e9dcb61`.
They contain the expected `knowledge/part-0000.md#node-000000` and the same four
additional IDs: node-019999, node-029999, node-099900, node-022000, with their
unchanged paths/coordinates/revision 4. All ten separate model-disabled FTS
outputs are byte-identical, SHA-256
`bbded559a0676ab0aeb14df493405b16483006ed03211df884736f0c1f72ee39`,
containing only node-000000 with revision 3. These are different fixtures and
remain separate series.

Recomputed medians from the per-call durations, checked all summary sample arrays,
then recomputed the speedup and percentage without using summary-derived inputs.
Chronological monotonic intervals confirm no measured-call overlap and the stated
hybrid order before/after, after/before, before/after. FTS order also alternates
over its five pairs. The FTS medians are 0.6825298750773072 and
0.6746258330531418 seconds (about 1.158% lower descriptively); the draft correctly
does not establish a material FTS improvement or pool it into hybrid evidence.

Recomputed CPU/RSS summary values directly from the six wait4 records:

| Metric | Before | After |
| --- | ---: | ---: |
| Median CPU seconds | 106.628184 | 57.269202 |
| Median peak RSS MiB | 180.5625 | 177.796875 |
| Maximum peak RSS MiB | 186.953125 | 180.640625 |

Before RSS bytes are [182353920, 196034560, 189333504]; after are
[181714944, 186433536, 189415424]. MiB uses 2^20. The records correctly identify
these as wait4 lifetime maxima, not simultaneous process-tree/whole-stack peaks;
Ollama/model memory is excluded. No exact allocator bound follows from these RSS
measurements. Startup help calls and model warmup are outside search durations.
The shared host and uncontrolled OS cache limitations are explicit; n=3/n=5 do
not establish tail latency or statistical generality.

## Method screening and conditioning

Checked all four screening binary hashes and 106 source hashes against their
per-method manifests. Relative to baseline, parser changes only scan.rs, memo
changes only search.rs, and combined changes exactly both production files.
Screening outputs match the final ordinary hybrid output. The n=1 durations are
117.01996212499216 (baseline), 91.07506554108113 (parser),
62.733120708027855 (memo), 61.14591208402999 (combined). They match the summary
and remain selection evidence, not extra final pairs or comparative medians.

The separate conditioning call lasted 116.61615895899013 seconds and has a saved
successful product response. The preserved wrapper traceback concerns the
exclusive writer creating measurement.json a second time. Its recovery receipt
references the exact raw-receipt hash
`f4fe2f3a73acde0da2946f8ad8dc4d2b9f8c8c04daf6ca57e334ee018e2b20be`;
that hash recomputes. The corrected driver uses raw-measurement.json and
measurement.json separately for actual screens/final calls. No evidence supports
classifying this wrapper error as a product failure; it is excluded from final n.

## Source, binary, verification, and corpus

Independently rehashed both executable files and all 106 recorded product inputs
for the baseline clone and actual final root. Comparison-manifest before/after
objects equal baseline-build.json/after-build.json. Only scan.rs and search.rs
production input hashes differ between before and after. The final root's 106
input hashes also equal the reviewed combined source capture.

- Before snapshot: `3d820de41ee12385bc734ac4ceeb56b78ba379f9`; executable SHA-256
  `9a3e2b70a907d135604ad04e6491e358d07575e987c1457d514a1820c2d2ba6a`.
- After captured snapshot: `2db40fd82fd8b504ebb2040b96b248031ce5d791`, verified
  as combined/source HEAD; executable SHA-256
  `bd2c79b6b3dff6858fd021ca7af521f9ceff735279844d76638f1879fa9fb0b6`.
- Actual root HEAD remains `52ed491a6688e0a4eee0cb5eceb3fc19f900465e` with intended
  uncommitted edits. The after snapshot identifies the isolated capture; the
  exact 106-byte-input equivalence is the link to the actual root.

Rehashed root-verify.log to its verification.json SHA-256
`5953367dc0f675b9555e24e1b874aadef4181d4f253a9d71c7ab9df0c610946b`.
Parsed all 171 successful test-result groups independently: 2423 passed
executions, zero failed, 22 ignored, and final `ok`. These are executions across
debug/release/example contexts, not unique tests. No suite was rerun by this audit.

Read final corpus/native-vector receipts. Verified canonical corpus manifest hash
`cf2235261a3ed884a0721080b1da745870c274998d74fff38f7e7f254c0cdd1f`, exact equality
of its 4334-entry input hash mapping with corpus-verification.json, and independently
rehashed every one of those files in the measured hybrid repository. All match.
The fresh native receipt reports 100000 keys and 100000 native vectors of dimension
768, with zero wrong widths, absent/orphan mappings, missing knowledge vectors,
recorded errors or reserved attempts. This separate native receipt supplies the
coverage evidence that vector_keys counts alone cannot provide. This reviewer did
not rerun its native SQL probe or reembed/reset the corpus.

## Native profile

Validated the compressed profile's size 679552 bytes and SHA-256
`b0f3682db7ff69eb88390506b8658bc797aaf7a943e2b5223bdd2735eefaacb9`.
Decoded raw samples: two threads, 65104 samples and 65104 usable stacks. Independently
resolved stack addresses with the saved symbol sidecar and aggregated each
sample's CPU delta once per function: all 1586 function totals/observation counts
match stacks.json, with observed CPU delta 60092177 microseconds.

`dwarfdump --uuid` on the final binary gives
`626044DD-83A3-3481-BB88-FAF8C4D8226A`, matching the profile library and symbol
sidecar code/debug IDs. The raw profile library path names the final pinned
product binary. Its receipt is explicitly profile-only, exit 0, with the same
hybrid stdout; it is not included in ordinary timing.

Recomputed new inclusive shares: file_parse 713499/60092177 = 1.1873409%;
layout_scan 234570/60092177 = 0.3903503%; historical hashing 35.1705431%; current
source hashing 14.5730567%; native vector retrieval 4.4688146%; CurrentKnowledge
build_link 34.6673761%. Earlier saved baseline profile values recompute to
53.6392876%, 31.6908552%, 20.0186983%, 8.4012620%, and 1.9026057% respectively.
The draft rounds them correctly and explicitly treats scopes as overlapping
inclusive CPU observations in different host contexts, not additive wall shares.

Result membership proves lexical-hit retention and additional vector contribution;
it cannot alone prove execution/causal contribution of the FTS branch. The saved
native data separately includes text_index_get_by_expression (25 stack
observations, 15010 microseconds) and fts5Bm25Function (21 observations, 14312
microseconds), plus native vector_index_get_by_vector/get_nearest_vectors frames.
Thus both retrieval paths are observed independently of result membership.
