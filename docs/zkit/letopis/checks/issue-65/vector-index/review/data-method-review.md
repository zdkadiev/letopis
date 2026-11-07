# Independent measurement-method preflight

2026-11-07. Read-only audit except this report. No builds, product tests, providers,
SQL/database probes, benchmarks, or full-vector/corpus hash scans were run. The
metadata-only native archive verifier was run, and compact saved records were
independently recomputed. This is preflight approval, not final campaign approval:
ordinary pairs/profile do not yet exist and the actual-root storage runner is
still coordinator-owned work.

## Verdict and scope

No open Critical/Important method finding after the three concrete preflight
corrections below. The revised drivers are suitable for the planned serialized
three-pair steady-state full CLI comparison. Native SQL evidence is internally
consistent, but its approximately20x result is not a product CLI speedup.

Reviewed final driver bytes and confirmed their documentation archive copies are
identical. Parsed both Python files with ast.parse without executing either:

| Driver | SHA-256 |
| --- | --- |
| measure-cli.py | 66d9a12406fc92e64d30988bf0779c67061d137945ea433716f909e01c76d78a |
| run-comparison.py | d46fcca218dd487170390ca48fe637f8c9c48b76493983b12d1f1981059ab4fc |

If these methods change before the campaign, the recorded driver hashes should
identify the actual executed versions and the delta needs scoped review.

## Resolved preflight findings

1. **Independent result oracle was too weak.** Initially the per-call check
   required the one literal lexical URI plus any four other known clean IDs.
   Final measure-cli.py asserts the exact five independently frozen IDs:
   node-000000 at part-0000, node-019999 at part-0666, node-029999 at part-0999,
   node-099900 at part-3330, and node-022000 at part-0733. It also requires five
   unique results, known corpus membership, clean flags, ok:true, exit0/success
   and no warnings. Final cross-call stdout hashes must all match, preserving
   before/after order, coordinates, revision and flags in the complete response.
2. **A retry could overwrite pre-pair evidence.** The original pairs-existence
   check did not protect a warmup created before the first pair directory. The
   revised driver reserves the campaign with exclusive creation before work,
   exclusively writes comparison-manifest and model-warmup, and creates a new
   mode-setups directory. Failed attempts cannot be silently overwritten by
   rerunning the same campaign. Per-call output directories likewise must be new.
3. **Setup failure evidence could be lost.** Mode setup used check=True and its
   receipt was written only after a later CLI success. The revised driver saves
   each mode setup's SQL, duration, stdout/stderr and exit/outcome immediately and
   exclusively in mode-setups before launching the CLI. Nonzero setup is then
   rejected. A bounded120-second timeout also saves a receipt before reraising.
   A later CLI assertion failure therefore cannot erase a successful setup record.

## Timer, order, identity and state checks

run-comparison selects the previously validated before snapshot
`2db40fd82fd8b504ebb2040b96b248031ce5d791` and the new after capture
`016eae8fa9705a209b01555a32877702b70cf74a`. It explicitly checks before106 and
after107 input entries, requires src/storage/vector_index.rs in after, requires
the intended after capture, and records both build manifests and driver hashes.
Each pinned executable and every source input are hashed before the campaign;
measure-cli repeats those checks before each call and checks executable identity
again afterward. At preflight the final after-build receipt was not yet present;
this is expected while the root runner/build is active. Its actual107 hashes and
binary identity must be verified in the later data audit, not assumed from guards.

The loop is sequential and implements before/after, after/before, before/after.
Each arm uses the same owned copied repository. Native none/flat mode rebuilding
finishes and its receipt is saved before the measurement helper is launched.
The helper verifies the selected cosine mode outside the timer. Every search is
a fresh process, using the same query case000000 and limit5 with the real enabled
embeddinggemma provider. A finite768-dimensional query-only provider warmup and
per-binary --help startup calls occur outside the timed searches. Proxy variables
are removed for the actual CLI's local provider calls; the direct warmup likewise
uses an explicitly empty proxy handler.

The timed region is only measure_process([binary,search], ...): process creation,
actual CLI refresh/storage/provider/search/output and process/pipe completion.
Its wait4 CPU/RSS refer to the child and OS-accounted reaped descendants, exclude
the separate Ollama daemon, and are not simultaneous whole-stack peak memory.
Raw output is saved before result assertions. Summary medians/speedup derive only
from the six ordinary records, not warmup/setup/conditioning/profile data.

Before each call the helper checks100000 knowledge IDs,100000 source nodes and
500000 edges, enabled provider config,100000 vector keys, zero failure rows and
the recorded embedding identity. Afterward it verifies the same vector key/error/
identity tuple and unchanged executable bytes. These counters are useful state
guards; vector_keys alone does not prove native payload completeness or byte
preservation. Separate native count/dimension/full-payload receipts remain the
appropriate evidence for those claims. No such307MB scans were repeated here.

The copied-fixture conditioning record is separately retained with conditioning
true:139.0452949590981 seconds, success, the exact five clean IDs, revision5.
Its raw/final equality, capture lengths and SHA-256 values recompute correctly.
It is excluded from the six records by the driver's explicit pairs loop. The
fixture-copy receipt records APFS cloning of files and SQLite online backup of
both databases rather than raw copying an active main database without WAL.
This audit inspected that receipt without repeating the copy or4334-file scan.

## Native diagnostic archive audit

Ran archive-verify.py successfully:270 archived originals,27 output receipts,
24 matched native runs, matching complete result prefixes, and saved full-vector
hash equality. The manifest records31409316 uncompressed bytes and13240881 stored
bytes, with original/compressed SHA-256 and deterministic gzip mtime0. It preserves
the failed v1 warmup and earlier formatting-only controls as separate evidence.
Generated README/verifier hashes are also checked. No database, WAL/SHM, dylib or
build is silently presented as part of this compact archive.

Independently recomputed every saved native arm's sample arrays, user+sys CPU,
medians and ratios from runs.json and confirmed all24 successful runs have one
timer, K output rows, and the stated alternating order for each K:

| K | None median SQL CPU s | Flat median SQL CPU s | Ratio |
| --- | ---: | ---: | ---: |
|5|0.087641|0.054451|1.60953885|
|100|0.088012|0.057254|1.53722011|
|1000|0.133266|0.056556|2.35635476|
|100000|2.145223|0.107106|20.02897130|

The raw scripts place .load/setup before .timer on. The primary SQLite timer
includes the SQL query and result serialization. Separate process measurements
include shell/extension overhead and must not be substituted for SQL timer CPU.
The accepted run set excludes the warmup whose output accidentally contained its
timer line; the corrected driver strips that timer into a distinct receipt and
requires exactly K cleaned data rows.

Verified the archived query's3072-byte length and SHA-256
`e33d6addade4f84c9c906edd41d2e8897046845b5453121dcdc370333341829a`, matching its
copy receipt. It is the stored embedding at rowid1, not the actual provider query
from the full CLI campaign. Both full-vector receipts report100000 rows and
307200000 payload bytes with the same ordered SHA3-256
`4ED319020B3ADFE2FD6FC375EE308B6266F2D9E74689A6DE1EF7E22488218EE4`.
Rehashed the small current pinned vec1 source and existing oracle against the
copy receipt; they match. The hash scans themselves were not rerun.

## Limits that remain explicit

The native experiment uses diagnostic SQLite CLI3.54.1 while the product embeds
SQLite3.53.2. It uses one saved stored vector and orders native rowids/distances;
it omits provider latency, product graph joins, refresh, provenance, filters,
eligible-channel fusion and knowledge-ID ties. The24 prefix comparisons and
full-vector receipts support this native fixture, not arbitrary corruption/error
equivalence or general numerical/relevance claims. Corrected nonfinite controls
use SQL types so NULL cannot be mistaken for printf-formatted numeric zero.

The final CLI method measures warm steady state after native setup, not first
opening/migration latency or cold storage. The per-call physical-mode rebuilds
themselves affect OS cache/layout outside the timer; the setup transition can
touch different pages in each arm. Cache state remains uncontrolled and must be
reported as such. Reversed pair order reduces order bias but does not turn three
samples on a shared host into tail-latency or production/minimum-hardware proof.

At this preflight no final ordinary durations/output equality, actual107-input
after pin, completed host runner, or final Samply result is claimed. Those require
the coordinator's subsequent artifacts and a separate final data audit.
