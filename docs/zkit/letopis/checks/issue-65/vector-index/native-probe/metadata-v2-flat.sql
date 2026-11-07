.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
select count(*) as keys from vector_keys;
select hex(val) as model_header from vector_index_model;
select count(*) as idx_blocks,sum(length(val)) as idx_bytes from vector_index_idx;
select count(*) as base_rows,sum(length(vector)) as base_bytes from vector_index_base;
select id,val from vector_index_config;
pragma quick_check;
