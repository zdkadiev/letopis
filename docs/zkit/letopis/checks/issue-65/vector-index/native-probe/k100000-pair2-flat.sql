.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode tabs
.output /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/vector-index-2026-11-07/native-probe/k100000-pair2-flat.raw
.timer on
select rowid,printf('%!.17g',distance) from vector_index(readfile('/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/vector-index-2026-11-07/native-probe/query.f32'),'{K:100000}') order by distance,rowid;
.timer off
.output stdout
