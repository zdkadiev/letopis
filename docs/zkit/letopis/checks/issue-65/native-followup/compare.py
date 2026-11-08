from pathlib import Path
import json,sys,os,time,statistics
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import hash_file,encode_json,write_json
from report import validate_result
from measure import measure_process
p=root/'.artifacts/issue-65/2026-11-06-native-followup'
builds={n:json.loads((p/f'{n}-build.json').read_text()) for n in ('before','after')}
repo=root/'.artifacts/issue-65/2026-11-06-100k-fts600/case-inspect-0'
corpus=json.loads((root/'.artifacts/issue-65/2026-11-06-100k-fts600/fixture/corpus.json').read_text())
for path,digest in corpus['input_sha256'].items():assert hash_file(repo/path)==digest,path
assert json.loads((repo/'.letopis/config.json').read_text())['embedding_model']['enabled'] is False
for b in builds.values():assert hash_file(Path(b['executable']))==b['sha256']
for path,digest in builds['after']['inputs'].items():assert hash_file(root/path)==digest,path
write_json(p/'comparison-manifest.json',{'builds':builds,'corpus':corpus,'repository':str(repo),'repetitions':3,'conditioning':'prior successful captured reads; no additional conditioning','cache_state':'OS caches uncontrolled; warm repeated fresh CLI processes','concurrency':'real Ollama sequential drain on separate copied corpus; shared host; no compilation/tests intentionally started during this campaign','started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())})
records=[]
with (p/'comparison-samples.jsonl').open('x') as stream:
 for pair in range(3):
  for command in ('check','status'):
   for side in (('before','after') if pair%2==0 else ('after','before')):
    b=builds[side];assert hash_file(Path(b['executable']))==b['sha256']
    r=measure_process([b['executable'],command],repo,encode_json({}),{'timeout':180})
    r.update(command=command,pair=pair,side=side,host_load=os.getloadavg(),validation=validate_result({'repository':repo,'expected':{'command':command,'counts':corpus['counts']}},r))
    stream.write(json.dumps(r)+'\n');stream.flush();os.fsync(stream.fileno());records.append(r)
    print(command,pair,side,round(r['duration_seconds'],4),r['validation'],flush=True)
    assert r['validation']['valid'] and r['outcome']=='success'
summary={}
for command in ('check','status'):
 values={side:[r['duration_seconds'] for r in records if r['command']==command and r['side']==side] for side in builds}
 medians={side:statistics.median(v) for side,v in values.items()}
 summary[command]={'samples':values,'median_seconds':medians,'speedup':medians['before']/medians['after'],'latency_reduction_percent':100*(1-medians['after']/medians['before'])}
write_json(p/'comparison-summary.json',summary);print(json.dumps(summary,indent=2))
