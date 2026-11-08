from pathlib import Path
import sys,json
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import prepare_existing_fixture,prepare_scenario,write_json
from measure import measure_process
from report import validate_result
base=root/'.artifacts/issue-65';evidence=base/'2026-11-05-profile-repeat';evidence.mkdir()
ordinary={'before':json.loads((base/'2026-11-05-smoke-before/build/build.json').read_text())['executable'],'after':str(base/'2026-11-05-optimized-build/letopis')}
instrument={'before':str(base/'2026-11-05-instrument/letopis-profiled'),'after':str(base/'2026-11-05-instrument-after/letopis-profiled')}
fixture=prepare_existing_fixture(base/'2026-11-05-survey-10k/fixture/corpus.json',evidence/'fixture');fixture['binary']=ordinary['before']
for side in ordinary:
 case=prepare_scenario(fixture,'inspect',evidence/f'case-{side}')
 setup=measure_process([ordinary[side],'inspect'],case['repository'],case['stdin'],{'timeout':120})
 setup['validation']=validate_result(case,setup);write_json(evidence/f'{side}-setup.json',setup)
 assert setup['validation']['valid']
 record=measure_process([instrument[side],'inspect'],case['repository'],case['stdin'],{'timeout':120})
 record['validation']=validate_result(case,record)
 record['timers']=[json.loads(line) for line in bytes.fromhex(record['stderr']['capture_hex']).decode().splitlines()]
 write_json(evidence/f'{side}.json',record)
 assert record['validation']['valid']
 print(side,record['duration_seconds'],[(x['function'].split('::')[-1],x['calls']) for x in record['timers'] if 'durability' in x['function']],flush=True)
