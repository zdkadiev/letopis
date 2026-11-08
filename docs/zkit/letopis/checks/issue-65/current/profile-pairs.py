from pathlib import Path
import sys,json,os,time
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import prepare_existing_fixture,prepare_scenario,write_json
from measure import measure_process
from report import validate_result
base=root/'.artifacts/issue-65';output=base/'2026-11-05-instrument-pairs-02';output.mkdir()
fixture=prepare_existing_fixture(base/'2026-11-05-survey-1k/fixture/corpus.json',output/'fixture')
before=json.loads((base/'2026-11-05-smoke-before/build/build.json').read_text())['executable']
instrument=str(base/'2026-11-05-instrument/letopis-profiled');fixture['binary']=before
records=[]
for pair in range(3):
 for label in (('ordinary','instrumented') if pair%2==0 else ('instrumented','ordinary')):
  case=prepare_scenario(fixture,'inspect',output/f'{pair}-{label}')
  record=measure_process([before if label=='ordinary' else instrument,'inspect'],case['repository'],case['stdin'],{'timeout':120})
  record.update(pair=pair,mode=label,load_average=os.getloadavg(),validation=validate_result(case,record))
  if label=='instrumented':record['timers']=[json.loads(x) for x in bytes.fromhex(record['stderr']['capture_hex']).decode().splitlines()]
  records.append(record);write_json(output/f'{pair}-{label}.json',record)
  print(label,record['duration_seconds'],record['validation']['valid'],flush=True)

write_json(output/'records.json',records)
