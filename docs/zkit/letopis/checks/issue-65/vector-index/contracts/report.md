# Exact vector acceleration: contract and populated-database integration

2026-11-07; source inspection only. No product edits, builds or benchmarks.
Pinned vec1 0.8 commit/checksum: `vendor/vec1/README.md:1–10`.

## Recommendation

Try **`index:"flat",distance:"cos"` with unchanged complete K and service
eligibility**, subject to native parity/corruption tests. This is a concrete
exact-search optimization, not ANN or a top-k contract change. It does not remove
the full-candidate observation cost. ANN needs a separately agreed relevance and
error-observation contract if it omits candidates; no such agreement follows
from merely enabling vector acceleration.

## Exact flat versus trained

The pinned implementation explicitly accepts `none` and `flat` text models
(`vendor/vec1/vec1.c:3238–3278`). Flat creates an indexed model with zero
codebooks and zero buckets (`11485–11489`), so it is an exhaustive unquantized
scan of packed full-vector blocks. Full vectors retain original bytes, without
normalization/quantization (`10470–10475`); flat computes `vec1CosDist` directly
on those original vectors (`8395–8424`). None generates SQL invoking
`vec1_cos_distance` (`9129–9142`), which calls the same `vec1CosDist` kernel
(`1285–1306`). Thus finite valid data has a source-backed route to identical
distances and final order. Keep complete K, SQL `order by v.distance,n.uri`, and
service metric/ID tie handling; native heap traversal order then cannot drop a
tie at a capped boundary (`src/storage/vector.rs:634–675`). Actual parity across
supported builds remains a test gate, not established by this inspection.

Trained models can have coarse buckets and quantizers. K does not control how
many buckets are visited: `vec1DoKANNQuery` scans `nProbe` buckets and can stop
earlier on `nprobe_slack` (`vec1.c:8469–8498`). Setting K to the whole corpus
therefore does **not** restore completeness with default probing. Probing all
buckets with no slack and no quantization would restore exhaustive full-vector
scanning, but pays training/coarse-selection overhead and gives no asymptotic
distance-work reduction. Quantized distances can still alter eligible channel
ranks even if every row is returned. No ANN benefit is established for this
complete-ranking contract.

## Existing populated databases are the main integration trap

Changing only `src/storage/vectors.sql:6` affects new databases. Bootstrap returns
early for recognized v1 schema (`schema.rs:79–95`); validation compares SQL
definitions and excludes native shadow tables (`118–162`), so it does not inspect
the persisted index model. The same-identity reset also returns without doing
anything (`vector.rs:430–435`). `RESET_INDEX` independently recreates none mode
(`vector.rs:26–29`). Both creation defaults and a populated-database upgrade are
needed.

Smallest proposed route: a private storage-owned mode-preparation operation,
called once at an existing writable opening/preparation boundary **before**
attaching/pinning consumer reads (`src/lib.rs:626–669`). Inspect the native model
through `vec1cat.model` (registered name at `vec1.c:12965–12969`): its source returns JSON `index`/`distance` for flat/none
(`vec1.c:12668–12718`). Avoid decoding a private binary header or adding an
application settings/cache table solely to mark this migration.

For known none/cos, acquire a normal storage write transaction, recheck after
waiting for the writer, and execute native `rebuild` to flat/cos. For already
flat/cos, do no write/rebuild. Unknown model, wrong metric or corruption must
produce a typed safe error rather than silently reset or repair it. Keep model
preparation separate from structural schema validation: existing recognized
schemas are validated on query-only connections (`schema.rs:338–348`), and
observational status must not rebuild (`src/lib.rs:674–693`). No public trait or
new configuration field is inherently required for this private physical mode.

Native rebuild retains existing rows and dimensions; it stores the new model,
increments native model version, reloads, and rebuilds (`vec1.c:11513–11542`),
with dimensions independently stored in config (`11383–11400`). **Flat moves
the authoritative vector payload into packed idx blocks** and base entries
become bucket references (`11159–11204`, `11836–11845`); vector readback follows
that representation (`9428–9457`). Never treat this as a disposable secondary
index and never delete/recreate it for an unchanged embedding identity.

Preserve exact rowids/search_hashes, meta.config_identity, failure attempts and
query_error. A physical mode change is not a model change. Preserve existing
idle-scope/reset checks and rollback-only behavior (`vector.rs:115–140`), native
dimension rejection (`502–549`), atomic key/vector insertion (`367–419`), and
native-first then key pruning (`217–265`). Config changes still clear/recreate
the vector index and keys atomically under the existing identity CAS
(`423–459`), now with flat/cos as reset default.

## Corruption and observation limits

Flat validates packed block size (`vec1.c:8373–8375`), whereas none's scalar
function validates blob types/lengths (`1298–1303`). Shared cosine arithmetic
does not prove equivalent corruption observation: heap handling of nonfinite
distances, missing/duplicate native rows, damaged blocks and native model/config
must be tested. Rebuild checks old blob lengths (`11191–11199`), but this path
does not visibly perform the insertion-time finite-value test (`11789–11794`).
Do not claim migration preserves every corruption case without explicit evidence.
Transactions must roll back failed rebuilds, and existing pinned readers must
continue seeing the old valid representation. Native xRollback unloads its model
and discards its writer (`vec1.c:11956–11959`); model unload clears cached version
and model (`5989–5995`). A fresh NN/vector-reading cursor reloads config and
compares persisted model version (`9212–9215`, `8556–8562`); `vec1cat.model`
always calls that loader (`12691`). Finish the mode-inspection statement before
rebuilding, and use a fresh completed inspection on every retry. **Do not retain
active cursors across rebuild/rollback:** cursor `bLoadConfig` is not cleared by
ResetCsr (`6338–6346`). The native source itself leaves a TODO about rebuild with
active readers (`11456`), so a separate-connection pinned-reader test is required;
this is not a guarantee established by the source alone. Bootstrap before consumer
attachments avoids same-connection read/capture invalidation.

Per-candidate eligibility cannot generally shrink compatibly. X-1-cli.md:58–67
requires complete eligible channels; `tests/knowledge_search.rs:310–335`
requires errors beyond final limit. Prior report/probe at
`.artifacts/issue-65/search-optimization-2026-11-06/ranking-contracts/` gives
literal native-K/overlap/filter counterexamples and first-demand/freshness rules.
Current search optimizations in the shared worktree are outside this report's
ownership; retain their accepted invariants while changing only physical vector
execution.

## Minimal implementation verification surface

1. Fresh empty flat/cos; populated none/cos upgrade with exact byte readback,
   rowids, dimensions, identity, attempts/query_error unchanged; reopen performs
   no rebuild; reset returns flat/cos and permits a new dimension.
2. None-versus-flat full candidate/metric/order parity: duplicate hashes,
   zero/signed-zero, ties, non-unit vectors, disjoint FTS/vector overlap and source
   filters; use literal expected winners, not solely mutual implementation parity.
3. Insert/delete/prune, rollback after insertion/rebuild failure, competing
   preparation, old pinned reader across rebuild, wrong identity and incompatible
   nested scope. Retain `tests/storage_vectors.rs:121–168,269–309` obligations.
4. Inject malformed old blobs and flat blocks, nonfinite metric inputs,
   missing native rows and config/model corruption; compare against the existing
   demanded-error contract, require no masked existing errors or partial links,
   and verify transaction release. Do not promote newly invented corruption
   coverage into an existing guarantee.
5. Update schema mirror/parity inputs and run mandated `make verify` plus the
   storage native/standalone runner for host and supported Linux target. No
   dependency/vendor source change is needed for the flat route.
