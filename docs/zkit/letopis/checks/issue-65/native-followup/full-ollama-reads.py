from pathlib import Path
import sys,json,sqlite3,subprocess,os,statistics
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import get_environment,encode_json,write_json,hash_file
from measure import measure_process
from report import validate_result
p=root/'.artifacts/issue-65/2026-11-06-native-followup';evidence=p/'ollama-100k';repo=evidence/'repository'
build=json.loads((p/'after-build.json').read_text());assert hash_file(Path(build['executable']))==build['sha256']
for path,digest in build['inputs'].items():assert hash_file(root/path)==digest,path
coverage=json.loads((evidence/'native-complete-coverage.json').read_text())[0]
assert coverage['keys']==coverage['native_vectors']==100000 and coverage['knowledge_without_vector']==0,coverage
assert all(coverage[k]==0 for k in ('wrong_vector_widths','keys_without_vector','vectors_without_key','orphan_hashes','recorded_errors','reserved_attempts')),coverage
c=sqlite3.connect((repo/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True)
known={x[0] for x in c.execute("select uri from nodes where kind in ('vertex','metavertex')")};c.close();assert len(known)==100000
lexical_sql="select n.uri from text_index join nodes n on n.id=text_index.rowid where text_index match '\"case\" AND \"000000\"'"
lexical=json.loads(subprocess.check_output(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',str(repo/'.letopis/cache.db'),lexical_sql],text=True))
assert lexical==[{'uri':'knowledge/part-0000.md#node-000000'}],lexical
lexical_ids={x['uri'] for x in lexical}
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
write_json(evidence/'full-reads-manifest.json',{'build':build,'coverage':coverage,'lexical_oracle':lexical,'oracle_sqlite':'/opt/homebrew/opt/sqlite/bin/sqlite3','cache_state':'already refreshed corpus and previously active model; cache state uncontrolled','scope':'full 100k-vector real Ollama mixed lexical/vector queries, no concurrent owned drain/tests/compilation; shared host; model resources excluded','timeout_seconds':300})
records=[]
for index in range(3):
 r=measure_process([build['executable'],'search'],repo,encode_json({'query':'case 000000','limit':5}),{'timeout':300,'environment':env})
 response=json.loads(bytes.fromhex(r['stdout']['capture_hex']));items=response.get('result',[]);ids={x['element_id'] for x in items}
 assert r['outcome']=='success' and response['ok'] and not response.get('warnings'),response
 assert len(items)==len(ids)==5 and ids.issubset(known) and lexical_ids.issubset(ids),items
 assert len(ids-lexical_ids)==4 and all(not x['flags'] for x in items),items
 r.update(validation={'valid':True,'oracle':'one independent FTS hit retained plus four nonlexical known URIs; proves both channels, no relevance-quality verdict'},load_average=os.getloadavg())
 write_json(evidence/f'full-hybrid-{index}.json',r);records.append(r);print('full hybrid',index,r['duration_seconds'],flush=True)
for command in ('check','status'):
 r=measure_process([build['executable'],command],repo,encode_json({}),{'timeout':180,'environment':env})
 r['validation']=validate_result({'repository':repo,'expected':{'command':command,'counts':{'knowledge':100000,'source':100000,'edges':500000}}},r)
 assert r['outcome']=='success' and r['validation']['valid'],r
 response=json.loads(bytes.fromhex(r['stdout']['capture_hex']))
 if command=='status':assert response['result']['embedding_backlog']==response['result']['embedding_failed']==0,response
 write_json(evidence/f'full-{command}.json',r);print('full',command,r['duration_seconds'],flush=True)
write_json(evidence/'full-hybrid-summary.json',{'samples_seconds':[r['duration_seconds'] for r in records],'median_seconds':statistics.median(r['duration_seconds'] for r in records),'n':3,'all_valid':True,'coverage':100000})
