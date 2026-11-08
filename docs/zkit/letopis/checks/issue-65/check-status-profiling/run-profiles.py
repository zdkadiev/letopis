from pathlib import Path
import sys,json,os,time
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import write_json,hash_file
from report import validate_result
from measure import measure_process
out=root/'.artifacts/issue-65/2026-11-06-check-status'
manifest=json.loads((root/'.artifacts/issue-65/2026-11-06-100k-direct/manifest.json').read_text())
ordinary=manifest['build'];instrument=json.loads((out/'cpu-build.json').read_text());repo=Path(manifest['repository'])
for build in (ordinary,instrument):assert hash_file(Path(build['executable']))==build['sha256']
for path,digest in ordinary['inputs'].items():assert hash_file(root/path)==digest,path
write_json(out/'campaign.json',{'ordinary':ordinary,'instrumented':instrument,'repository':str(repo),'repetitions':3,'order':'alternating ordinary/instrumented per command','started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'cache_state':'initialized and OS page cache uncontrolled; new CLI process each call','scope':'FTS configuration; no provider calls; inclusive wall and thread CPU profiling'})
with (out/'profiles.jsonl').open('x') as stream:
 for name in ('check','status'):
  scenario={'repository':repo,'expected':{'command':name,'counts':{'knowledge':100000,'source':100000,'edges':500000}}}
  for pair in range(3):
   for mode in (('ordinary','instrumented') if pair%2==0 else ('instrumented','ordinary')):
    build=ordinary if mode=='ordinary' else instrument
    result=measure_process([build['executable'],name],repo,b'{}\n',{'timeout':120,'capture_limit':262144})
    result.update(command=name,pair=pair,mode=mode,load_average=os.getloadavg(),validation=validate_result(scenario,result))
    if mode=='instrumented':
     assert not result['stderr']['truncated']
     result['profile']=json.loads(bytes.fromhex(result['stderr']['capture_hex']))
     assert result['profile']['cpu_available']
    stream.write(json.dumps(result)+'\n');stream.flush();os.fsync(stream.fileno())
    print(name,pair,mode,round(result['duration_seconds'],3),result['validation']['valid'],flush=True)
    assert result['validation']['valid']
write_json(out/'completion.json',{'outcome':'complete'})
