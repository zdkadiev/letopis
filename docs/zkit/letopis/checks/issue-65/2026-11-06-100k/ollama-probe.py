from pathlib import Path
import sys,json,sqlite3,os
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import prepare_fixture,write_json,encode_json,get_environment
from contention import embed_lease
from measure import measure_process
base=root/'.artifacts/issue-65';evidence=base/'2026-11-06-ollama-probe'
binary=base/'2026-11-05-optimized-build/product/letopis'
fixture=prepare_fixture(evidence/'fixture',60);repo=fixture['repository']
config={'embedding_model':{'enabled':True,'provider':'ollama','model':'embeddinggemma:300m-qat-q4_0','parameters':{'base_url':'http://127.0.0.1:11434/api'},'max_retries':0}}
(repo/'.letopis/config.json').write_text(json.dumps(config)+'\n')
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
for name,args,request,lease in [('index',['index'],{},True),('query',['search'],{'query':'"case 000000"','limit':1},True),('drain',['embed','--drain'],{},False),('status',['status'],{},False)]:
 def invoke():
  result=measure_process([str(binary),*args],repo,encode_json(request),{'timeout':20,'environment':env})
  result['response']=json.loads(bytes.fromhex(result['stdout']['capture_hex']).decode()) if result['stdout']['bytes'] else None
  write_json(evidence/f'product-{name}.json',result)
  print(name,result['outcome'],result['response'],flush=True)
 if lease:
  with embed_lease(repo):invoke()
 else:invoke()
connection=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
coverage={'vectors':connection.execute('select count(*) from vector_keys').fetchone()[0],'failures':connection.execute('select count(*) from embed_failures').fetchone()[0]}
connection.close();write_json(evidence/'coverage.json',coverage);print(coverage)
