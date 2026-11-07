.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
attach 'file:/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository/.letopis/cache.db?mode=ro' as cache;
select strftime('%Y-%m-%dT%H:%M:%fZ','now') as captured_utc,
       (select count(*) from vector_keys) as keys,
       (select count(*) from vector_index) as native_vectors,
       (select val from vector_index_config where id=2) as dimensions,
       (select count(*) from vector_index where length(vector)<>3072) as wrong_vector_widths,
       (select count(*) from (select row_id from vector_keys except select rowid from vector_index)) as keys_without_vector,
       (select count(*) from (select rowid from vector_index except select row_id from vector_keys)) as vectors_without_key,
       (select count(*) from cache.nodes n where n.kind<>'source' and not exists(select 1 from vector_keys k where k.search_hash=n.search_hash)) as knowledge_without_vector,
       (select count(*) from vector_keys k where not exists(select 1 from cache.nodes n where n.search_hash=k.search_hash and n.kind<>'source')) as orphan_hashes,
       (select count(*) from embed_failures where error is not null) as recorded_errors,
       (select count(*) from embed_failures where error is null) as reserved_attempts;
