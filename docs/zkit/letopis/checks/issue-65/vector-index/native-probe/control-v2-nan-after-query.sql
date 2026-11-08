.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
select rowid,distance,typeof(distance) as distance_sql_type,printf('%!.17g',distance) as formatted_distance from vector_index(x'0000803f00000000','{K:3}') order by distance,rowid;
