.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
attach '/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/bounded-search-2026-11-07/repository/.letopis/vectors.db' as vec;
select n.uri as id,bm25(text_index) as metric
from text_index join nodes n on n.id=text_index.rowid
where text_index match '"case" AND "000000"' and n.kind <> 'source'
order by metric,n.uri limit 5;
