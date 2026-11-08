.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
select hex(sha3_query('select rowid, vector from vector_index order by rowid',256)) as ordered_vector_sha3_256;
select count(*) as rows,sum(length(vector)) as vector_bytes from vector_index;
