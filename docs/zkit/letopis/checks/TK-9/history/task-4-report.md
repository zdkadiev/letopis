# TK-9 Task 4 implementation report

Status: complete, ready for parent review. Implementation commit: `dbbf0ef43cab3550f77e8869200e047c40986b2f`. Base: `9f201423122aee4ba7d1ca11a6294e0ee9e20b02`. Branch/worktree: TK-9, `/Users/zdkadiev/.codex/worktrees/tk-9/letopis`. Runtime: Darwin ARM64; Rust 1.95.0 (`59807616e`). Parent-owned specification/plan/task edits were preserved and excluded from the implementation commit. Parent owns Engram.

## Implemented contract

Added VectorOutcome, StoredVectorKey, VectorMatch, scoped VectorWrite and Storage/StorageRead methods. Per controller ruling, the verb-form method is `find_nearest_vectors`, replacing the brief's initial `nearest_vectors` spelling. No behavior expansion follows that rename.

SqliteStorage now retains exactly three connections: attached read, unattached cache writer, unattached vector writer. The vector unit takes BEGIN IMMEDIATE, rechecks the expected identity before exposing the capability, and keeps that lock through commit. Failed validation or native SQL poisons the whole unit; explicit commit rejects it and RAII rolls it back. Successful writes INSERT OR IGNORE a hash key, preserve an existing native vector, reject an orphan existing key, and remove the hash's embedding failure in the same transaction. f32 conversion uses safe native-endian bytes.

Reset takes a separate Immediate transaction, rereads identity, returns false unchanged, otherwise recreates native vec1 with no index/cosine options and clears keys/failures before replacing meta. query_error is preserved. Cache revision is untouched. Read identity checks use each handle's meta identity captured in its own pinned transaction. Native nearest requests all vector_keys candidates and joins through the read snapshot's current nodes.search_hash; results sort by distance then knowledge ID.

## Test-first evidence

Before writing implementation, added seven integration behavior scenarios. `cargo test --locked --offline --test storage_vectors` failed as expected: absent VectorOutcome/VectorWrite imports and absent vector Storage/StorageRead methods, 31 compile errors. The task explicitly requested missing-API RED. The initial tests called `nearest_vectors`; the controller subsequently required its verb-form rename, applied consistently before implementation/green execution.

The minimal implementation then passed all seven scenarios. Added private transaction rollback/corruption/cleanup checks, plus stronger populated old/new nearest and ordering/query-validation assertions. These additional checks passed on their first execution and were not reported as independent RED cycles. Final vector integration count is eight, with two additional private unit scenarios.

## Exact behavior assertions and mutation targets

- Shared text: a.md and b.md share digest(shared), one native [1,0,0] vector, repeated storage returns the same row_id with inserted=false without replacing bytes; native search returns exactly both IDs at distance0. Cache revision remains1 after vector writes.
- Identity guard: digest(M1) is current; captured digest(M2) rejects both write capability and vector reads. Guarded nearest returns ConfigChanged before inspecting even an invalid empty query.
- Interleaving: A publishes a.md with hash M1, stores [1,0,0], opens and actually queries a pinned M1 reader (native nearest and exact vector). B independently resets to digest(M2), stores [0,1,0,0], and publishes a.md with M2 hash. A still returns a.md/M1 at distance0 and exactly [1,0,0]. A fresh B reader returns a.md/M2 at distance0 and exactly [0,1,0,0], rejects captured M1, and cannot read old M1 vector under M2. Repeating the M2 reset returns false and retains populated M2 bytes.
- Current hash: old hash has [1,0,0], new hash has [0,1,0]; a.md with the new hash matches only that new vector at distance1. An obsolete distance0 vector never substitutes for edited text.
- Poisoning: each of [], NaN, infinity and length4/dimensions3 follows an earlier valid pair in one unit; caught error followed by commit fails, and both hashes read absent. Dropped units roll back vector/failure cleanup.
- Late native rejection: seed dimension3 then insert a new hash with valid finite length4/dimensions4; native vec1 fails after the key insertion. Catching the error cannot commit; SQL confirms one original key only; exact old bytes survive and rejected hash is absent.
- Reset rollback: private SqliteStorage test installs BEFORE UPDATE ON meta RAISE(ABORT). Reset has already recreated vec1 and cleared keys/failures when metadata update fails. Assert M1 identity, original key, exact [1,0,0] and failure row survive. Drop the trigger and close all handles; reopening still sees M1/exact vector. A successful M2 reset then clears failures and retains query_error='safe failure'. No runtime fault switch or rusqlite hooks feature added.
- Independence/rebuild: two Storage instances simultaneously hold cache and vector write units on different files, both commit. Close handles, remove only temporary cache, reopen with existing vectors file: same hash/key is reused, inserted=false, and rebuilt cache revision remains0.
- Native query behavior: malformed queries reject; valid length4 query against dimension3 fails natively. An unused closest vector cannot cause early candidate truncation: all three joined results return exactly z.md/distance0, a.md/distance1, b.md/distance1. Orphan key is an error and poisons commit. Failure deletion is transactional and durable only after successful commit.

## Verification

Final `./scripts/check-rust.sh`: exit0, formatting/check/Clippy all targets pass, 99 root tests pass (12 unit +87 integration), doc-tests 0. No warnings. Final `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`: exit0. `git diff --check`: exit0. Focused `cargo test --locked --offline --test storage_vectors`: 8/8 pass. Private focused run: 2/2 pass.

## Self-review and limits

Reviewed physical files, visibility, associated capability implementations and dependency direction. Concrete SqliteStorage/SqliteVectorWrite/SqliteRead own their implementations; service types contain no SQLite; SQL remains private in storage. Existing lib.rs constructs SqliteStorage, which now constructs and retains the dedicated vector writer. No provider calls, retry/reservation orchestration, CLI/ranking stubs, async, pool or ORM added. No attached writer transaction or unsafe/self-referential handle added. Functions/tests remain below 60 lines, imports are explicit, public fallible contracts document Errors. Parent performs independent review.

No known implementation blocker. This run covers macOS ARM64 only; Linux/x86_64, physical power-loss and historical separate-manifest harnesses were not run by this Task4 worker. Task6 owns broader product/static-link platform evidence. No merge/push performed.

## Final source SHA-256

- `src/service/mod.rs`: `b07d7718315bda383dff9dcdc4a301ba7e59bcefd849b3dae80c48e3f462825f`
- `src/service/model.rs`: `8f726ed131bc48a4e25fffd184b27e0cd39a41e3a38bb61350cd945bb8379b0a`
- `src/storage/mod.rs`: `56d1b0d62aa6d80be3ecf1c3f396b04589136d9f36d9c20c8d16e990ff76b8b8`
- `src/storage/sqlite.rs`: `109a00afbd028077c23e4c76465e07a96bb21439423bf5a2dafd8078dda75c00`
- `src/storage/read.rs`: `7b8424d4c1ec9e5f61863000cf0a2c39b0f6ba56898e411015f8769f353d109f`
- `src/storage/vector_write.rs`: `6831c99729a3c17ca2f34ea39ef47f63c1d8886753a16c23aabc657f2fb1a8c1`
- `src/storage/vectors.rs`: `65c3a7dc89a8c8049af7cb78387d523ef151eaa6acd0f469e9906f5c8b035820`
- `tests/storage_vectors.rs`: `2c36077bd3254c7db268e6497ed0d85f2a37f8c1646c4e2cb3c8b1aefd78e0ec`

## RED output (expected missing API)

```text
   Compiling letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
error[E0432]: unresolved imports `letopis::service::VectorOutcome`, `letopis::service::VectorWrite`
 --> tests/storage_vectors.rs:3:89
  |
3 | use letopis::{domain::Sha256, open_storage, service::{CacheWrite, Storage, StorageRead, VectorOutcome, VectorWrite}};
  |                                                                                         ^^^^^^^^^^^^^  ^^^^^^^^^^^ no `VectorWrite` in `service`
  |                                                                                         |
  |                                                                                         no `VectorOutcome` in `service`

error[E0599]: no method named `begin_vector_write` found for mutable reference `&mut impl Storage` in the current scope
  --> tests/storage_vectors.rs:10:43
   |
10 |     let mut unit = unwrap_current(storage.begin_vector_write(identity).unwrap());
   |                                           ^^^^^^^^^^^^^^^^^^
   |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
  --> src/service/mod.rs:63:5
   |
63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:22:21
   |
22 |     assert!(storage.reset_vectors(&identity).unwrap());
   |                     ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:27:43
   |
27 |     let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
   |                                           ^^^^^^^^^^^^^^^^^^
   |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
  --> src/service/mod.rs:63:5
   |
63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:33:36
   |
33 |     assert_eq!(unwrap_current(read.read_vector(&identity, &hash).unwrap()), Some(vec![1.0,0.0,0.0]));
   |                                    ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `nearest_vectors` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:34:39
   |
34 |     let matches = unwrap_current(read.nearest_vectors(&identity, &[1.0,0.0,0.0], 3).unwrap());
   |                                       ^^^^^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:45:13
   |
45 |     storage.reset_vectors(&m1).unwrap();
   |             ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:47:30
   |
47 |     assert!(matches!(storage.begin_vector_write(&m2).unwrap(), VectorOutcome::ConfigChanged));
   |                              ^^^^^^^^^^^^^^^^^^
   |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
  --> src/service/mod.rs:63:5
   |
63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:49:27
   |
49 |     assert!(matches!(read.read_vector(&m2, &m1).unwrap(), VectorOutcome::ConfigChanged));
   |                           ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `nearest_vectors` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:50:27
   |
50 |     assert!(matches!(read.nearest_vectors(&m2, &[], 0).unwrap(), VectorOutcome::ConfigChanged));
   |                           ^^^^^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:62:11
   |
62 |     first.reset_vectors(&m1).unwrap();
   |           ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:65:35
   |
65 |     assert_eq!(unwrap_current(old.read_vector(&m1, &m1).unwrap()), Some(vec![1.0,0.0,0.0]));
   |                                   ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:66:20
   |
66 |     assert!(second.reset_vectors(&m2).unwrap());
   |                    ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:68:21
   |
68 |     assert!(!second.reset_vectors(&m2).unwrap());
   |                     ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:69:35
   |
69 |     assert_eq!(unwrap_current(old.read_vector(&m1, &m1).unwrap()), Some(vec![1.0,0.0,0.0]));
   |                                   ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:71:28
   |
71 |     assert!(matches!(fresh.read_vector(&m1, &m1).unwrap(), VectorOutcome::ConfigChanged));
   |                            ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:72:37
   |
72 |     assert_eq!(unwrap_current(fresh.read_vector(&m2, &m2).unwrap()), Some(vec![0.0,1.0,0.0,0.0]));
   |                                     ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:73:37
   |
73 |     assert_eq!(unwrap_current(fresh.read_vector(&m2, &m1).unwrap()), None);
   |                                     ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
  --> tests/storage_vectors.rs:80:13
   |
80 |     storage.reset_vectors(&m1).unwrap();
   |             ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `nearest_vectors` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
  --> tests/storage_vectors.rs:87:39
   |
87 |     let matches = unwrap_current(read.nearest_vectors(&m1, &[1.0,0.0,0.0], 3).unwrap());
   |                                       ^^^^^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:101:17
    |
101 |         storage.reset_vectors(&m1).unwrap();
    |                 ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:102:47
    |
102 |         let mut unit = unwrap_current(storage.begin_vector_write(&m1).unwrap());
    |                                               ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
   --> src/service/mod.rs:63:5
    |
 63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
   --> tests/storage_vectors.rs:107:40
    |
107 |         assert_eq!(unwrap_current(read.read_vector(&m1, &hash).unwrap()), None);
    |                                        ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
   --> tests/storage_vectors.rs:108:40
    |
108 |         assert_eq!(unwrap_current(read.read_vector(&m1, &m1).unwrap()), None);
    |                                        ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:118:13
    |
118 |     storage.reset_vectors(&m1).unwrap();
    |             ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:120:43
    |
120 |     let mut unit = unwrap_current(storage.begin_vector_write(&m1).unwrap());
    |                                           ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
   --> src/service/mod.rs:63:5
    |
 63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
   --> tests/storage_vectors.rs:127:36
    |
127 |     assert_eq!(unwrap_current(read.read_vector(&m1, &m1).unwrap()), Some(vec![1.0,0.0,0.0]));
    |                                    ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `read_vector` found for associated type `<impl Storage as Storage>::Read<'_>` in the current scope
   --> tests/storage_vectors.rs:128:36
    |
128 |     assert_eq!(unwrap_current(read.read_vector(&m1, &hash).unwrap()), None);
    |                                    ^^^^^^^^^^^ method not found in `<impl Storage as Storage>::Read<'_>`

error[E0599]: no method named `reset_vectors` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:138:11
    |
138 |     first.reset_vectors(&identity).unwrap();
    |           ^^^^^^^^^^^^^ method not found in `impl Storage`

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:140:49
    |
140 |     let mut vector_unit = unwrap_current(second.begin_vector_write(&identity).unwrap());
    |                                                 ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
   --> src/service/mod.rs:63:5
    |
 63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `begin_vector_write` found for opaque type `impl Storage` in the current scope
   --> tests/storage_vectors.rs:148:43
    |
148 |     let mut unit = unwrap_current(rebuilt.begin_vector_write(&identity).unwrap());
    |                                           ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `begin_cache_write` with a similar name, but with different arguments
   --> src/service/mod.rs:63:5
    |
 63 |     fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Some errors have detailed explanations: E0432, E0599.
For more information about an error, try `rustc --explain E0432`.
error: could not compile `letopis` (test "storage_vectors") due to 31 previous errors
```

## Focused GREEN output

```text
   Compiling letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.57s
     Running tests/storage_vectors.rs (target/debug/deps/storage_vectors-fa0da5eb883374c6)

running 8 tests
test dimension_reset_and_rollback ... ok
test text_hash_shares_one_vector ... ok
test current_node_hash_excludes_old_vector ... ok
test vectors_require_matching_identity ... ok
test nearest_validates_query_and_orders_all_candidates_by_distance_then_id ... ok
test cache_and_vector_writers_are_independent ... ok
test old_reader_survives_reset_and_late_reader_rejects_captured_old_identity ... ok
test caught_vector_error_cannot_commit_key_or_prior_write ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.49s

```

## Private focused output

```text
   Compiling letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.47s
     Running unittests src/lib.rs (target/debug/deps/letopis-bbfd9e64c18a55e3)

running 2 tests
test storage::sqlite::tests::success_deletes_failure_atomically_and_corrupt_key_poisoning_rolls_back ... ok
test storage::sqlite::tests::reset_late_meta_failure_preserves_identity_key_vector_and_query_error ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 0.17s

```

## Final full-check output

```text
    Checking letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.19s
    Checking letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.29s
   Compiling letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.82s
     Running unittests src/lib.rs (target/debug/deps/letopis-bbfd9e64c18a55e3)

running 12 tests
test git::cli::tests::version_gate ... ok
test storage::native::tests::native_capabilities_match_pinned_stack ... ok
test storage::connection::tests::both_database_connections_enforce_policy ... ok
test storage::cache_write::tests::caught_sql_failure_poisoning_restores_metadata_and_index ... ok
test storage::sqlite::tests::success_deletes_failure_atomically_and_corrupt_key_poisoning_rolls_back ... ok
test storage::sqlite::tests::reset_late_meta_failure_preserves_identity_key_vector_and_query_error ... ok
test git::cli::tests::open_rejects_non_repository_and_bare ... ok
test git::cli::tests::process_removes_inherited_overrides_and_hardens_writes ... ok
test git::cli::tests::discovers_root_from_subdirectory ... ok
test git::cli::tests::subprocess_drains_and_reaps_on_io_failure ... ok
test git::cli::tests::open_rejects_old_git_before_object_calls ... ok
test git::cli::tests::discovers_trailing_newline_repository_without_touching_sibling ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.27s

     Running tests/domain_values.rs (target/debug/deps/domain_values-620d67b029d62e45)

running 7 tests
test edge_targets_and_self_loops_are_checked ... ok
test edge_identity_and_kind_are_derived ... ok
test sha256_hex_round_trip_and_invalid_inputs ... ok
test anchors_and_ranges_reject_invalid_values ... ok
test ids_and_sources_normalize_from_git_root ... ok
test tree_oids_are_full_lowercase_hex ... ok
test sha256_uses_exact_bytes ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/git_retention.rs (target/debug/deps/git_retention-634f6a34fe6c4286)

running 11 tests
test conflicting_descendant_types_do_not_pin ... ok
test shared_clone_is_refused_before_donor_gc_can_break_retention ... ok
test alternates_added_after_open_refuse_operations_without_writes ... ok
test unretained_unique_tree_is_collectable ... ok
test concurrent_pin_is_idempotent ... ok
test gitlinks_are_retained_without_dereferencing ... ok
test promisor_miss_never_fetches ... ok
test incomplete_tree_and_ref_failure_do_not_pin ... ok
test linked_worktree_checks_common_object_store_and_refuses_inactive_alternates ... ok
test symbolic_and_conflicting_refs_are_rejected ... ok
test retained_snapshots_survive_gc ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.33s

     Running tests/git_snapshots.rs (target/debug/deps/git_snapshots-2a04b859e12e0dc6)

running 5 tests
test empty_tree_and_invalid_paths ... ok
test replacement_refs_do_not_change_bytes ... ok
test tree_types_and_missing_objects_are_distinct ... ok
test paths_are_literal_and_never_shell_input ... ok
test exact_bytes_without_index_or_commit ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.26s

     Running tests/markdown_elements.rs (target/debug/deps/markdown_elements-c9010d80cf40e981)

running 10 tests
test unmarked_markdown_is_not_corpus_and_invalid_utf8_is_rejected ... ok
test metadata_must_fit_exactly_one_standalone_line ... ok
test anchors_are_unique_across_real_headings ... ok
test code_examples_do_not_declare_elements ... ok
test snapshot_metadata_preserves_full_oid_and_zero_created_at ... ok
test nested_elements_and_orphan_metadata_are_rejected ... ok
test unicode_crlf_and_eof_have_exact_spans ... ok
test raw_html_content_does_not_declare_metadata ... ok
test file_errors_never_return_partial_elements ... ok
test golden_elements_have_exact_bytes_and_metadata ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/markdown_links.rs (target/debug/deps/markdown_links-9e1f6072a4ac075b)

running 6 tests
test image_alt_links_do_not_declare_edges ... ok
test links_outside_elements_are_only_navigation ... ok
test links_wrapping_images_still_declare_edges ... ok
test sources_are_normalized_without_loading_destinations ... ok
test only_inline_links_declare_edges ... ok
test invalid_graph_titles_and_targets_fail_file ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/markdown_search.rs (target/debug/deps/markdown_search-419746c5e3f8e263)

running 8 tests
test reference_labels_use_whole_file_context ... ok
test moving_path_preserves_text_hashes ... ok
test code_examples_remain_searchable ... ok
test crlf_metadata_line_does_not_add_search_whitespace ... ok
test own_text_label_and_code_change_search_hash ... ok
test search_text_preserves_word_and_block_boundaries ... ok
test metadata_and_link_changes_keep_search_hash ... ok
test golden_search_representations ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/storage_bootstrap.rs (target/debug/deps/storage_bootstrap-ba0e64613360acef)

running 14 tests
test non_utf8_attachment_path_is_rejected ... ok
test missing_parent_and_nul_paths_are_rejected ... ok
test owned_storage_outlives_path_arguments ... ok
test independent_thread_connections_register_native_vec1 ... ok
test same_database_path_is_rejected ... ok
test literal_unicode_quote_and_question_mark_paths_open ... ok
test symlink_and_hardlink_aliases_are_rejected ... ok
test sqlite_like_prefix_does_not_hide_extra_user_tables ... ok
test parallel_initializers_leave_a_complete_schema ... ok
test busy_bootstrap_returns_typed_error_without_retry ... ok
test fresh_storage_reopens_with_empty_identity ... ok
test malformed_stored_observations_return_safe_typed_errors ... ok
test vector_schema_and_unversioned_user_data_are_not_recreated ... ok
test unknown_or_partial_schema_is_not_recreated ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s

     Running tests/storage_cache.rs (target/debug/deps/storage_cache-0ffadcf304b5dfa5)

running 18 tests
test late_fts_error_rolls_back_all_changes ... ok
test dropped_unit_rolls_back ... ok
test caught_mutation_error_poisoned_unit_cannot_commit ... ok
test duplicate_edges_and_invalid_checks_are_rejected ... ok
test fts_matches_use_native_score_then_id_without_candidate_limit ... ok
test source_missing_bytes_and_declared_targets_round_trip ... ok
test empty_cache_publications_preserve_vector_identity_and_state ... ok
test contentless_fts_does_not_return_original_text ... ok
test revision_overflow_rolls_back ... ok
test malformed_persisted_values_return_internal_errors ... ok
test incoming_edges_survive_target_delete_and_reappear ... ok
test cache_commit_preserves_node_row_id ... ok
test knowledge_and_source_optional_fields_survive_reopen ... ok
test deleting_node_removes_fts_and_refuses_remaining_outgoing_edges ... ok
test unsigned_values_do_not_wrap ... ok
test source_time_and_line_overflow_poison_the_unit ... ok
test valid_invalid_and_unmarked_file_rows_round_trip ... ok
test source_projection_can_be_recomputed_and_removed ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s

     Running tests/storage_vectors.rs (target/debug/deps/storage_vectors-fa0da5eb883374c6)

running 8 tests
test text_hash_shares_one_vector ... ok
test dimension_reset_and_rollback ... ok
test current_node_hash_excludes_old_vector ... ok
test nearest_validates_query_and_orders_all_candidates_by_distance_then_id ... ok
test vectors_require_matching_identity ... ok
test old_reader_survives_reset_and_late_reader_rejects_captured_old_identity ... ok
test cache_and_vector_writers_are_independent ... ok
test caught_vector_error_cannot_commit_key_or_prior_write ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s

   Doc-tests letopis

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

```

## Strict Rustdoc output

```text
 Documenting letopis v0.0.0 (/Users/zdkadiev/.codex/worktrees/tk-9/letopis)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s
   Generated /Users/zdkadiev/.codex/worktrees/tk-9/letopis/target/doc/letopis/index.html
```
