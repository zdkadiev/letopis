# Independent final code and contract review

Reviewed the actual uncommitted diff against `52ed491a6688e0a4eee0cb5eceb3fc19f900465e` on 2026-11-06. Scope: all five changed Git Rust files; all six changed performance-runner Python files; README, current specification and implementation plan; relevant X-3/I-4 contracts and mechanics. Also traced surrounding reader, hashing, repository ownership, closure-audit and cleanup code. No product files or Git metadata were changed.

## Findings

Critical: none.

Important: none.

Minor: none requiring action.

## Code and contract assessment

- `ObjectReader::tree_bytes_get_by_oid` obtains fresh raw bytes through the existing exact-type and independently hashed identity path before consulting `validated_trees`. Missing objects, decode errors, wrong types and hash failures therefore retain the prior ordering and caller-selected classification. Successful structural validation is the sole insertion path. Errors are not cached, and restoration is usable immediately.
- The memo stores only validated lowercase full OID strings. Their width is checked before insertion, and insertion stops at 4096 identities. Overflow still validates and returns valid objects. No payload cache or global lifetime was introduced. Separate readers start empty; cloned repositories intentionally share their command-owned reader.
- `refs.rs` still independently walks the complete closure and directly calls structural validation on every visited tree for each strong pin. Its per-audit visited set, type-conflict check and blob verification are unchanged; it does not consult the structural memo.
- Loose-object reuse computes the requested identity with the existing collision-detecting SHA-1/SHA-256 implementation, reads and independently verifies the existing loose body/type, checks shared-repository configuration, syncs the existing file and parent chain, and checks cancellation before and after confirmation. Packed-only/absent objects retain staged publication. Cancellation of an absent probe is caught by subsequent stage admission. Safe diagnostics avoid raw payload and object-store pathname disclosure.
- The existing pathname/replacement race boundary is unchanged and explicitly acknowledged by the specification. This is not evidence of stronger hostile-filesystem or power-loss guarantees.
- The performance runner now invokes `index --full` for both the historical `reindex` scenario and contention writer, while validating the current `index` result. Keeping the old oracle name supported preserves historical records. Positive revision validation rejects booleans and nonintegers.
- Changes remain inside their existing Git and measurement responsibilities. There are no new application traits, dependencies, public composition changes, runtime processes, SQL changes or infrastructure orchestration across component boundaries.

## Verification and coverage assessed

The five memo tests use independently written Git objects and cover both SHA formats, reuse, reader isolation, decodable hash corruption, wrong type, disappearance, restoration, invalid structure, full-audit bypass prevention and bounded overflow. Existing raw-reader decoding and corruption tests remain on the delegated path. The loose-reuse tests cover SHA formats, tree/blob identities, packed fallback, corruption, cancellation and injected file/directory sync failures. Python regression tests pin argv, JSON input and result validation.

Inspected the supplied Git-test receipt: 57 passed, 0 failed. Inspected five mutation receipts: each reported exit 101 and detected=true. These are execution evidence supplied by the implementation run; this reviewer did not rerun a full suite or independently replay mutations. Ran `git diff --check` during this review: success, no output. Required root `make verify` was reported in progress when the review started; this review does not claim its completion. Native performance attribution and final before/after measurements remain the measurement report's responsibility, not a claim established by code inspection.

Assessment: no code/contract blocker found in the reviewed diff. Completion remains conditional on the orchestrator confirming the final required checks and measurement/report obligations against the actual final tree.
