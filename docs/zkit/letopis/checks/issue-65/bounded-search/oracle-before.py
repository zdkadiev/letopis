from pathlib import Path
import json,subprocess,struct
base=Path(__file__).parent;root=Path('/Users/zdkadiev/Repos/letopis');out=base/'oracle';fixture=base/'repository'
v=json.loads((out/'query-vector.json').read_text())['values'];hexvector=struct.pack('<768f',*v).hex();ext=root/'.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib'
head=f".load {ext} sqlite3_extension_init\nattach '{fixture}/.letopis/vectors.db' as vec;\n"
for limit in [5,100]:
 sql=f'''with
text_hits as materialized (
 select n.uri as id,bm25(text_index) as metric from text_index join nodes n on n.id=text_index.rowid
 where text_index match '"case" AND "000000"' and n.kind <> 'source'
),
vector_hits as materialized (
 select n.uri as id,v.distance as metric from vec.vector_index(X'{hexvector}','{{K:100000}}') v
 join vec.vector_keys k on k.row_id=v.rowid join nodes n on n.search_hash=k.search_hash
 where n.kind <> 'source'
),
weights as (
 select id,1.0 / row_number() over (order by metric,id) as weight from text_hits
 union all
 select id,1.0 / row_number() over (order by metric,id) as weight from vector_hits
)
select id,sum(weight) as score from weights group by id order by score desc,id limit {limit};'''
 p=out/f'{limit}-before.json';assert not p.exists()
 (out/f'{limit}-before.sql').write_text(head+sql+'\n')
 r=subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',str(fixture/'.letopis/cache.db')],input=head+sql,text=True,capture_output=True,check=True)
 rows=json.loads(r.stdout);p.write_text(json.dumps({'expected_ids':[x['id'] for x in rows],'scope':'Independent SQL full-channel reciprocal-rank oracle; current synthetic corpus clean; outside timed CLI.'},indent=2)+'\n')
 print(limit,len(rows),[x['id'] for x in rows[:5]],flush=True)
