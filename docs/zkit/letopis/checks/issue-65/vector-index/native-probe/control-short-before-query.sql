.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
select rowid,printf('%!.17g',distance) as distance from vector_index(x'0000803f00000000','{K:3}') order by distance,rowid;
