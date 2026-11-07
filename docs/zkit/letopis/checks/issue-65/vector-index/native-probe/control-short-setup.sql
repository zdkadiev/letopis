.bail on
.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
.mode json
create virtual table vector_index using vec1(vector);
insert into vector_index(cmd,arg) values('rebuild','{index:"none",distance:"cos"}');
insert into vector_index(rowid,vector) values(1,x'0000803f00000000'),(2,x'000000000000803f'),(3,x'0000803f0000803f');
update vector_index_base set vector=x'000080' where id=2;
