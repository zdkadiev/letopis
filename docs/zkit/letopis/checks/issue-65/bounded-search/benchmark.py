"""Fresh exclusive ordinary measurements; never overwrite historical samples."""
from pathlib import Path
import sys,os,json,hashlib,statistics,sqlite3,time,fcntl,subprocess,urllib.request,struct
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;fixture=base/'repository'
sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
lock=(base/'campaign.lock').open('a+b')
fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
out=base/'pairs';out.mkdir(exist_ok=False)
(out/'reservation.json').write_text(json.dumps({'pid':os.getpid(),'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'exclusive_output':str(out),'fixture_lock':str(base/'campaign.lock'),'protocol':'Atomic mkdir refuses existing campaign; nonblocking flock held until process exit; per-call compiler/test/profile/letopis process checks; no external writer prevention implied.'},indent=2)+'\n')
host=json.loads((base/'host-storage/manifest.json').read_text());assert host['status']=='PASS'
for path,digest in host['inputs'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
pin=json.loads((base/'product/pin.json').read_text())
binaries={'before':root/'.artifacts/issue-65/vector-index-2026-11-07/product/letopis','after':base/'product/letopis'}
hashes={'before':'ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9','after':pin['sha256']}
env=dict(os.environ)
for n in ['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy']:env.pop(n,None)
env.update(NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def state():
 with sqlite3.connect((fixture/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True) as c:
  vector=c.execute('select (select count(*) from vector_keys),(select count(*) from embed_failures),(select config_identity from meta where id=1)').fetchone()
  dimensions=c.execute('select val from vector_index_config where id=2').fetchone()[0]
  native_config=c.execute('select id,typeof(val),hex(val) from vector_index_config order by id').fetchall()
  native_model_sha256=hashlib.sha256(c.execute('select val from vector_index_model where id=1').fetchone()[0]).hexdigest()
  query_errors=c.execute('select count(*) from query_error').fetchone()[0]
 with sqlite3.connect((fixture/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True) as c:
  graph=c.execute("select (select count(*) from nodes where kind <> 'source'),(select count(*) from nodes where kind='source'),(select count(*) from edges),(select revision from project where id=1)").fetchone()
 return {'vectors':vector,'graph':graph,'dimensions':dimensions,'native_config':native_config,'native_model_sha256':native_model_sha256,'query_errors':query_errors,'config_sha256':sha(fixture/'.letopis/config.json')}
initial=state();assert initial['vectors'][:2]==(100000,0) and initial['graph'][:3]==(100000,100000,500000)
assert initial['dimensions']==768 and initial['query_errors']==0
canonical_path=root/'.artifacts/issue-65/2026-11-06-100k-fts600/fixture/corpus.json'
canonical=json.loads(canonical_path.read_text())
for path,digest in canonical['input_sha256'].items():assert sha(fixture/path)==digest,path
freshness=json.loads((base/'freshness.json').read_text());assert freshness['verified_knowledge']==101 and freshness['verified_bindings']==101
provider=json.loads((base/'provider.json').read_text())
method_paths=[Path(__file__),base/'oracle.py',base/'oracle-before.py',base/'freshness.py',base/'condition.py',base/'freshness.json',base/'provider.json',canonical_path,root/'tests/performance/measure.py',root/'tests/performance/scenarios.py',root/'.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib',Path('/opt/homebrew/opt/sqlite/bin/sqlite3').resolve(),*sorted((base/'oracle').iterdir())]
method_pins={str(path.resolve()):sha(path) for path in method_paths if path.is_file()}
(out/'method-pins.json').write_text(json.dumps(method_pins,indent=2)+'\n')
def assert_quiescent():
 active=[]
 for line in subprocess.check_output(['ps','-axo','pid=,ppid=,comm='],text=True).splitlines():
  fields=line.split(None,2)
  if len(fields)==3 and Path(fields[2]).name in {'cargo','rustc','samply','letopis','storage_check'}:active.append({'pid':int(fields[0]),'ppid':int(fields[1]),'executable':fields[2]})
 assert not active,active
 return active
def check_provider():
 with urllib.request.build_opener(urllib.request.ProxyHandler({})).open('http://127.0.0.1:11434/api/tags',timeout=10) as response:models=json.load(response)['models']
 assert next(m['digest'] for m in models if m['name']==provider['model']['name'])==provider['model']['digest']
assert_quiescent();check_provider()
warm_started=time.monotonic()
warm_request=urllib.request.Request('http://127.0.0.1:11434/api/embed',data=json.dumps(provider['request']).encode(),headers={'Content-Type':'application/json'})
with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(warm_request,timeout=90) as response:warm=json.load(response)
query=warm['embeddings'][0];assert len(query)==768
assert hashlib.sha256(struct.pack('<768f',*query)).hexdigest()==provider['vector_f32_sha256']
(out/'warmup.json').write_text(json.dumps({'outside_cli_timers':True,'seconds':time.monotonic()-warm_started,'model_digest':provider['model']['digest'],'query_f32_sha256':provider['vector_f32_sha256']},indent=2)+'\n')
# Verify the old captured source separately from the current changed tree.
before_pin=json.loads((root/'.artifacts/issue-65/vector-index-2026-11-07/product/after-build.json').read_text())
before_source=root/'.artifacts/issue-65/vector-index-2026-11-07/candidate/source'
for path,digest in before_pin['inputs'].items():assert sha(before_source/path)==digest,path
(out/'baseline-source-pin.json').write_text(json.dumps({'source':str(before_source),'inputs':before_pin['inputs'],'sha256':hashes['before']},indent=2)+'\n')
for label,binary in binaries.items():assert sha(binary)==hashes[label]
for path,digest in pin['inputs'].items():assert sha(root/path)==digest,path
(out/'manifest.json').write_text(json.dumps({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'binaries':{k:str(v) for k,v in binaries.items()},'sha256':hashes,'source_inputs':pin['inputs'],'state':initial,'fixture':str(fixture),'model':'embeddinggemma:300m-qat-q4_0','scope':'Fresh CLI processes; warm model/OS caches uncontrolled; no owned compilation/tests/drain/profile during ordinary pairs; wait4 excludes Ollama daemon. Semantic changes are validated against separate SQL oracles, not assumed byte equal.'},indent=2)+'\n')
all_results=[]
def run(label,limit,name,omit=False):
 directory=out/name;directory.mkdir(exist_ok=False)
 assert sha(binaries[label])==hashes[label]
 active=assert_quiescent();check_provider()
 payload={'query':'case 000000'}
 if not omit:payload['limit']=limit
 expected_limit=min(limit,100)
 expected=json.loads((base/'oracle'/f'{expected_limit}-expected.json').read_text())['expected_ids']
 if label=='before':expected=json.loads((base/'oracle'/f'{expected_limit}-before.json').read_text())['expected_ids']
 receipt=measure_process([str(binaries[label]),'search'],fixture,json.dumps(payload).encode(),{'timeout':240,'environment':env,'capture_limit':1048576})
 (directory/'measurement.json').write_text(json.dumps(receipt,indent=2)+'\n')
 assert receipt['outcome']=='success' and receipt['exit_code']==0,receipt
 assert not receipt['stdout']['truncated']
 response=json.loads(bytes.fromhex(receipt['stdout']['capture_hex']));assert response['ok'];links=response['result']
 assert [x['element_id'] for x in links]==expected,(name,[x['element_id'] for x in links],expected)
 assert all(not x['flags'] and x['revision']==initial['graph'][3] for x in links),name
 assert state()==initial and sha(binaries[label])==hashes[label]
 record={'name':name,'label':label,'limit':expected_limit,'request':payload,'seconds':receipt['duration_seconds'],'cpu_seconds':receipt['resources']['cpu_seconds']['value'],'peak_rss_bytes':receipt['resources']['peak_rss']['value'],'stdout_sha256':receipt['stdout']['sha256'],'validated':True,'matching_tool_processes_before_call':active,'config_and_vector_metadata_unchanged':True}
 (directory/'validation.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record),flush=True);return record
# Screening is retained separately and excluded from paired medians.
for limit in [5,100]:run('after',limit,f'screening-{limit}')
for limit in [5,100]:
 for pair in range(3):
  order=['before','after'] if pair%2==0 else ['after','before']
  for label in order:all_results.append(run(label,limit,f'{limit}-{pair}-{label}'))
run('after',100,'smoke-default',omit=True);run('after',1000,'smoke-clamped')
summary={}
for limit in [5,100]:
 summary[str(limit)]={}
 for label in ['before','after']:
  rows=[x for x in all_results if x['limit']==limit and x['label']==label]
  summary[str(limit)][label]={'n':len(rows),'wall_samples_seconds':[x['seconds'] for x in rows],'wall_median_seconds':statistics.median(x['seconds'] for x in rows),'cpu_median_seconds':statistics.median(x['cpu_seconds'] for x in rows),'max_cli_peak_rss_bytes':max(x['peak_rss_bytes'] for x in rows),'stdout_sha256':[x['stdout_sha256'] for x in rows]}
 b=summary[str(limit)]['before']['wall_median_seconds'];a=summary[str(limit)]['after']['wall_median_seconds'];summary[str(limit)]['speedup']=b/a;summary[str(limit)]['latency_reduction_percent']=(b-a)/b*100
(base/'comparison-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for path,digest in pin['inputs'].items():assert sha(root/path)==digest,path
for path,digest in method_pins.items():assert sha(Path(path))==digest,path
for path,digest in canonical['input_sha256'].items():assert sha(fixture/path)==digest,path
check_provider()
print(json.dumps(summary,indent=2),flush=True)
