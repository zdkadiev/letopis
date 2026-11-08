from pathlib import Path
import json,urllib.request,struct,subprocess,math,hashlib
base=Path(__file__).parent;root=Path('/Users/zdkadiev/Repos/letopis');fixture=base/'repository';out=base/'oracle';out.mkdir(exist_ok=False)
config=json.loads((fixture/'.letopis/config.json').read_text())['embedding_model']
request=urllib.request.Request('http://127.0.0.1:11434/api/embed',data=json.dumps({'model':config['model'],'input':['case 000000']}).encode(),headers={'Content-Type':'application/json'})
with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request,timeout=90) as r:response=json.load(r)
vector=response['embeddings'][0];assert len(vector)==768 and all(math.isfinite(x) for x in vector)
blob=struct.pack('<768f',*vector);(out/'query-vector.json').write_text(json.dumps({'model':config['model'],'query':'case 000000','values':vector,'f32_sha256':hashlib.sha256(blob).hexdigest()},indent=2)+'\n')
ext=root/'.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib'
heads=f".load {ext} sqlite3_extension_init\nattach '{fixture}/.letopis/vectors.db' as vec;\n"
for limit in [5,100]:
 channels={}
 for channel,sql in [('fts',f'''select n.uri as id,bm25(text_index) as metric
from text_index join nodes n on n.id=text_index.rowid
where text_index match '"case" AND "000000"' and n.kind <> 'source'
order by metric,n.uri limit {limit};'''),('vectors',f'''select n.uri as id,v.distance as metric
from vec.vector_index(X'{blob.hex()}','{{K:100000}}') v
join vec.vector_keys k on k.row_id=v.rowid join nodes n on n.search_hash=k.search_hash
where n.kind <> 'source' order by v.distance,n.uri limit {limit};''')]:
  (out/f'{limit}-{channel}.sql').write_text(heads+sql+'\n')
  r=subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',str(fixture/'.letopis/cache.db')],input=heads+sql,text=True,capture_output=True,check=True)
  (out/f'{limit}-{channel}.json').write_text(r.stdout);channels[channel]=json.loads(r.stdout or '[]');assert len(channels[channel])<=limit
 from fractions import Fraction
 scores={}
 for hits in channels.values():
  for rank,hit in enumerate(hits,1):scores[hit['id']]=scores.get(hit['id'],Fraction())+Fraction(1,rank)
 ranked=sorted(scores,key=lambda x:(-scores[x],x))[:limit]
 (out/f'{limit}-expected.json').write_text(json.dumps({'limit':limit,'channel_counts':{k:len(v) for k,v in channels.items()},'unique_candidates':len(scores),'expected_ids':ranked,'scope':'Independent native SQL top-L channels and exact rational reciprocal-rank oracle; immutable synthetic corpus flags independently clean.'},indent=2)+'\n')
 print(limit,len(scores),ranked[:5],flush=True)
