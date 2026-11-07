.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
select hex(sha3_query('select rowid, vector from vector_index order by rowid')) as payload_sha3;
