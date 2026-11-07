from pathlib import Path
import sys,json,sqlite3,time,os,urllib.request
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import get_environment,encode_json,write_json,hash_file
from measure import measure_process
base=root/'.artifacts/issue-65/2026-11-06-native-followup'
evidence=base/'ollama-100k';repo=evidence/'repository'
build=json.loads((base/'before-build.json').read_text());binary=build['executable']
assert hash_file(Path(binary))==build['sha256']
corpus=json.loads((root/'.artifacts/issue-65/2026-11-06-100k-fts600/fixture/corpus.json').read_text())
for path,digest in corpus['input_sha256'].items():assert hash_file(repo/path)==digest,path
config={'embedding_model':{'enabled':True,'provider':'ollama','model':'embeddinggemma:300m-qat-q4_0','parameters':{'base_url':'http://127.0.0.1:11434/api'},'max_retries':0}}
(repo/'.letopis/config.json').write_text(json.dumps(config)+'\n')
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
models=json.load(urllib.request.urlopen('http://127.0.0.1:11434/api/tags',timeout=5))
write_json(evidence/'manifest.json',{'build':build,'config':config,'model':next(x for x in models['models'] if x['name']==config['embedding_model']['model']),'counts':corpus['counts'],'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'timeout_seconds':3600,'scope':'real local Ollama; CLI wait4 resources exclude daemon/model; host shared with compilation/tests, OS/model caches uncontrolled','load_average':os.getloadavg(),'input_hashes_verified':True})
print('Starting real 100k drain',flush=True)
r=measure_process([binary,'embed','--drain'],repo,encode_json({}),{'timeout':3600,'environment':env})
write_json(evidence/'drain.json',r)
print('Drain',r['outcome'],r['duration_seconds'],flush=True)
c=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
counts={'vectors':c.execute('select count(*) from vector_keys').fetchone()[0],'failures':c.execute('select count(*) from embed_failures').fetchone()[0]}
c.close();write_json(evidence/'coverage.json',counts);print(counts,flush=True)
assert r['outcome']=='success' and counts=={'vectors':100000,'failures':0}
assert json.loads(bytes.fromhex(r['stdout']['capture_hex']))['ok']
