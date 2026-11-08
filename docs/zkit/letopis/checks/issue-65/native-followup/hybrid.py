from pathlib import Path
import sys,json,sqlite3,os,time
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import get_environment,encode_json,write_json,hash_file
from measure import measure_process
p=root/'.artifacts/issue-65/2026-11-06-native-followup';evidence=p/'ollama-100k';repo=evidence/'repository'
label=sys.argv[1]; build=json.loads((p/'before-build.json').read_text()); assert hash_file(Path(build['executable']))==build['sha256']
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
query='zirconiumsemanticprobe65'
c=sqlite3.connect((repo/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True)
known_ids={row[0] for row in c.execute("select uri from nodes where kind in ('vertex','metavertex')")};c.close()
assert len(known_ids)==100000
def coverage():
 c=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
 result={'vectors':c.execute('select count(*) from vector_keys').fetchone()[0],'failures':c.execute('select count(*) from embed_failures').fetchone()[0]};c.close();return result
# Independent oracle: none of the knowledge bytes contain the vector-channel probe token.
assert not any(query.encode() in file.read_bytes().lower() for file in (repo/'knowledge').glob('*.md'))
for index in range(3):
 before=coverage();r=measure_process([build['executable'],'search'],repo,encode_json({'query':query,'limit':5}),{'timeout':180,'environment':env})
 response=json.loads(bytes.fromhex(r['stdout']['capture_hex']));items=response.get('result',[])
 assert r['outcome']=='success' and response['ok'] and not response.get('warnings'),response
 assert len(items)==5 and len({x['element_id'] for x in items})==5,items
 assert all(not x['flags'] and x['element_id'] in known_ids for x in items),items
 # With no lexical token in any document, these five candidates require successful query inference and vector retrieval.
 r.update(validation={'valid':True,'oracle':'independent raw-byte lexical absence, five unique known knowledge IDs with clean flags'},query=query,coverage_before=before,coverage_after=coverage(),scope='synthetic real-provider vector-only candidates; no relevance-quality claim; CLI resources exclude model')
 write_json(evidence/f'{label}-hybrid-{index}.json',r);print(label,index,r['duration_seconds'],before,[x['element_id'] for x in items],flush=True)
