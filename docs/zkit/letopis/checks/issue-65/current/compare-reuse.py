from pathlib import Path
import json, sys, os, time, statistics
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import prepare_existing_fixture,prepare_scenario,write_json,hash_file
from measure import measure_process
from report import validate_result
base=root/'.artifacts/issue-65';evidence=base/'2026-11-05-reuse-comparison';evidence.mkdir()
before=json.loads((base/'2026-11-05-smoke-before/build/build.json').read_text())['executable']
after=str(base/'2026-11-05-optimized-build/letopis')
write_json(evidence/'manifest.json',{'before':before,'after':after,'hashes':{p:hash_file(Path(p)) for p in (before,after)},'cache':'OS page cache uncontrolled; fresh CLI process per call','timing':'setup excluded; alternating pairs; no concurrent intentional workloads','started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())})
records=[]
with (evidence/'samples.jsonl').open('x') as stream:
 for scale in (1000,10000):
  fixture=prepare_existing_fixture(base/f'2026-11-05-survey-{scale//1000}k/fixture/corpus.json',evidence/f'fixture-{scale}');fixture['binary']=before
  for name in (('inspect','search','check','refresh_bulk','reindex') if scale==1000 else ('inspect',)):
   mutation=name in ('refresh_bulk','reindex');cases={}
   for pair in range(3 if mutation else 12):
    for side in (('before','after') if pair%2==0 else ('after','before')):
     key=f'{scale}-{name}-{side}-{pair}'
     if mutation or side not in cases:cases[side]=prepare_scenario(fixture,name,evidence/key)
     case=cases[side]
     record=measure_process([before if side=='before' else after,*case['argv']],case['repository'],case['stdin'],{'timeout':120})
     record.update(nodes=scale,name=name,side=side,pair=pair,phase='measured' if mutation or pair>=2 else 'conditioning',load_average=os.getloadavg(),validation=validate_result(case,record))
     stream.write(json.dumps(record)+'\n');stream.flush();os.fsync(stream.fileno());records.append(record)
     print(scale,name,side,pair,round(record['duration_seconds'],4),record['validation']['valid'],flush=True)
     if not record['validation']['valid']:raise ValueError(record['validation'])
summary=[]
for scale,name in sorted({(x['nodes'],x['name']) for x in records}):
 row={'nodes':scale,'name':name}
 for side in ('before','after'):
  samples=[x for x in records if x['nodes']==scale and x['name']==name and x['side']==side and x['phase']=='measured']
  values=[x['duration_seconds'] for x in samples]
  row[side]={'n':len(samples),'median':statistics.median(values),'min':min(values),'max':max(values),'peak_rss_bytes':max(x['resources']['peak_rss']['value'] for x in samples),'cpu_median_seconds':statistics.median(x['resources']['cpu_seconds']['value'] for x in samples),'values':values}
 row['reduction_percent']=100*(1-row['after']['median']/row['before']['median'])
 summary.append(row)
write_json(evidence/'summary.json',summary)
print(json.dumps(summary,indent=2))
