from pathlib import Path
import sys,json,hashlib,time,os,signal
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import build_request,build_traversal_oracle,build_inspection_oracle,encode_json,write_json,hash_file,FIRST_ID
from report import validate_result,summarize_samples
from measure import measure_process
base=root/'.artifacts/issue-65';evidence=base/'2026-11-06-100k-direct'
prior=base/'2026-11-06-100k-fts600'
repo=prior/'case-inspect-0'
corpus=json.loads((prior/'fixture/corpus.json').read_text())
build=json.loads((base/'2026-11-05-optimized-build/build.json').read_text())
assert hash_file(Path(build['executable']))==build['sha256']
for path,digest in build['inputs'].items():assert hash_file(root/path)==digest,path
for path,digest in corpus['input_sha256'].items():assert hash_file(repo/path)==digest,path
assert json.loads((repo/'.letopis/config.json').read_text())['embedding_model']['enabled'] is False
write_json(evidence/'manifest.json',{'build':build,'repository':str(repo),'corpus_manifest':str(prior/'fixture/corpus.json'),'counts':corpus['counts'],'binary_hash_verified':True,'product_input_hashes_verified':True,'corpus_hashes_verified':True,'cache_state':'OS page cache uncontrolled; already initialized and captured repository; fresh CLI process per invocation','model_scope':'FTS only; separate real-Ollama probe blocked by sandbox networking','selected_scenarios':['search','traverse','inspect','check','status','reindex'],'repetitions':'3 per read scenario, 1 full index','started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'host_load_average':os.getloadavg()})
records=[];previous=signal.signal(signal.SIGTERM,lambda *_:(_ for _ in ()).throw(KeyboardInterrupt()))
try:
 with (evidence/'samples.jsonl').open('x') as stream:
  for name in ('search','traverse','inspect','check','status','reindex'):
   command,request=build_request(name)
   expected={'command':command,'id':FIRST_ID,'counts':corpus['counts']}
   if name=='inspect':expected['inspection']=build_inspection_oracle(repo,FIRST_ID)
   if name=='traverse':expected.update(build_traversal_oracle())
   scenario={'repository':repo,'expected':expected}
   for index in range(1 if name=='reindex' else 3):
    argv=['index','--full'] if name=='reindex' else [command]
    record=measure_process([build['executable'],*argv],repo,encode_json(request),{'timeout':300 if name=='reindex' else 120})
    record.update(name=name,nodes=100000,phase='measured',embedding_mode='fts_disabled',profile_mode='none',sample=index,validation=validate_result(scenario,record),load_average=os.getloadavg())
    stream.write(json.dumps(record)+'\n');stream.flush();os.fsync(stream.fileno());records.append(record)
    print(name,index,round(record['duration_seconds'],4),record['outcome'],record['validation'],flush=True)
    if not record['validation']['valid']:raise ValueError('invalid or censored sample')
 write_json(evidence/'summary.json',{'series':[dict(name=name,**summarize_samples([x for x in records if x['name']==name])) for name in dict.fromkeys(x['name'] for x in records)]})
 write_json(evidence/'completion.json',{'outcome':'complete'})
except BaseException as error:
 write_json(evidence/'completion.json',{'outcome':'interrupted' if isinstance(error,KeyboardInterrupt) else 'failed','error_type':type(error).__name__})
 raise
finally:signal.signal(signal.SIGTERM,previous)
