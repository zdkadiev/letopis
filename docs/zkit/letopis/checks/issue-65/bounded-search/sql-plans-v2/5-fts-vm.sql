.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
attach '/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/bounded-search-2026-11-07/repository/.letopis/vectors.db' as vec;
.explain off
.parameter init
insert into temp.sqlite_parameters(key,value) values('?1','"case" AND "000000"'),('?2','source'),('?3',5);
explain select nodes.uri, nodes.id, bm25(text_index) as score
                 from text_index
                 join nodes on nodes.id = text_index.rowid
                 where text_index match ?1 and nodes.kind <> ?2
                 order by score, nodes.uri
                 limit ?3;
