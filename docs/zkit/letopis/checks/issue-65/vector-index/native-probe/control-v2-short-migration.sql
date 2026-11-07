.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
insert into vector_index(cmd,arg) values('rebuild','{index:"flat",distance:"cos"}');
