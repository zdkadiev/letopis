.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
select rowid,hex(vector) as bytes from vector_index order by rowid;
select hex(val) as model from vector_index_model;
