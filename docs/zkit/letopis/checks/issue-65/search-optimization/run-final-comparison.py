from pathlib import Path
import sys,subprocess,json,statistics,os,time,urllib.request,math,hashlib
root=Path.cwd();p=root/'.artifacts/issue-65/search-optimization-2026-11-06'
before=json.loads((p/'baseline-build.json').read_text());after=json.loads((p/'after-build.json').read_text())
variants={'before':(Path(before['executable']),root/'.artifacts/issue-65/2026-11-06-native-followup/source'),'after':(Path(after['executable']),root)}
assert not (p/'comparison-manifest.json').exists(), 'use a fresh evidence campaign'
pins={'before':before,'after':after}
def verify_pin(side):
 binary,source=variants[side];pin=pins[side]
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==pin['sha256']
 for name,digest in pin['inputs'].items():assert hashlib.sha256((source/name).read_bytes()).hexdigest()==digest,(side,name)
for side in variants:verify_pin(side)

# Keep model-loading time outside ordinary CLI comparison; this stores no document vector.
config=json.loads((root/'.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository/.letopis/config.json').read_text())['embedding_model']
request=urllib.request.Request(config['parameters']['base_url']+'/embed',data=json.dumps({'model':config['model'],'input':'case 000000'}).encode(),headers={'Content-Type':'application/json'},method='POST');started=time.monotonic()
response=json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request,timeout=60));assert len(response['embeddings'])==1 and len(response['embeddings'][0])==768 and all(math.isfinite(x) for x in response['embeddings'][0])
(p/'comparison-model-warmup.json').write_text(json.dumps({'duration_seconds':time.monotonic()-started,'model':config['model'],'dimensions':768,'scope':'single direct query warmup outside measured CLI calls; no vector database write'},indent=2)+'\n')
manifest={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'before':before,'after':after,'repetitions':3,'order':'before/after; after/before; before/after','scope':'ordinary fresh CLI calls, same full100k populated corpus, no intentionally concurrent owned build/test/profile/drain; shared host, OS cache uncontrolled','model_warmup':'comparison-model-warmup.json'}
(p/'comparison-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
records=[]
for pair in range(3):
 for side in (('before','after') if pair%2==0 else ('after','before')):
  verify_pin(side);binary,source=variants[side];out=p/'pairs'/f'{pair}-{side}'
  subprocess.run([sys.executable,'-B',str(p/'measure-search.py'),'--binary',str(binary),'--source',str(source),'--output',str(out),'--label',side,'--phase','measured'],cwd=root,check=True)
  record=json.loads((out/'measurement.json').read_text());record['pair']=pair;record['side']=side;records.append(record)
summary={side:{'samples_seconds':[r['duration_seconds'] for r in records if r['side']==side],'median_seconds':statistics.median(r['duration_seconds'] for r in records if r['side']==side)} for side in variants}
summary.update(speedup=summary['before']['median_seconds']/summary['after']['median_seconds'],latency_reduction_percent=100*(1-summary['after']['median_seconds']/summary['before']['median_seconds']),valid_calls=len(records),n_per_side=3)
(p/'comparison-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2),flush=True)
# Cheap narrow-FTS control on the model-disabled fixture, kept as a separate series.
fts=[]
for pair in range(5):
 for side in (('before','after') if pair%2==0 else ('after','before')):
  verify_pin(side);binary,source=variants[side];out=p/'fts'/f'{pair}-{side}'
  subprocess.run([sys.executable,'-B',str(p/'measure-search.py'),'--binary',str(binary),'--source',str(source),'--output',str(out),'--label',side,'--phase','measured','--mode','fts'],cwd=root,check=True)
  r=json.loads((out/'measurement.json').read_text());r['side']=side;fts.append(r)
(p/'fts-summary.json').write_text(json.dumps({side:{'n':5,'samples_seconds':[r['duration_seconds'] for r in fts if r['side']==side],'median_seconds':statistics.median(r['duration_seconds'] for r in fts if r['side']==side)} for side in variants},indent=2)+'\n')
print('Final ordinary comparisons complete',flush=True)
