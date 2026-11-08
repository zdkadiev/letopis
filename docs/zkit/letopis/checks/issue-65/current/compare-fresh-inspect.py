from pathlib import Path
import json,sys,os,statistics
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import prepare_existing_fixture,prepare_scenario,write_json
from measure import measure_process
from report import validate_result
base=root/'.artifacts/issue-65';evidence=base/'2026-11-05-fresh-inspect';evidence.mkdir()
binaries={'before':json.loads((base/'2026-11-05-smoke-before/build/build.json').read_text())['executable'],'after':str(base/'2026-11-05-optimized-build/letopis')}
fixture=prepare_existing_fixture(base/'2026-11-05-survey-10k/fixture/corpus.json',evidence/'fixture');fixture['binary']=binaries['before']
records=[]
for pair in range(3):
 for side in (('before','after') if pair%2==0 else ('after','before')):
  case=prepare_scenario(fixture,'inspect',evidence/f'{pair}-{side}')
  # Each copy has never captured knowledge into the Git object store.
  record=measure_process([binaries[side],'inspect'],case['repository'],case['stdin'],{'timeout':120})
  record.update(pair=pair,side=side,load_average=os.getloadavg(),validation=validate_result(case,record))
  write_json(evidence/f'{pair}-{side}.json',record);records.append(record)
  print(side,pair,record['duration_seconds'],record['validation']['valid'],flush=True)
  if not record['validation']['valid']:raise ValueError(record['validation'])
write_json(evidence/'records.json',records)
print({side:statistics.median(x['duration_seconds'] for x in records if x['side']==side) for side in binaries})
