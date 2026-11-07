from pathlib import Path
import json,sys
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
from report import validate_result
from scenarios import write_json,hash_file
base=root/'.artifacts/issue-65';evidence=base/'2026-11-06-100k-check-profile';evidence.mkdir()
repo=Path(json.loads((base/'2026-11-06-100k-direct/manifest.json').read_text())['repository'])
build=json.loads((base/'2026-11-05-instrument-after/build.json').read_text())
assert hash_file(Path(build['executable']))==build['sha256']
scenario={'repository':repo,'expected':{'command':'check','counts':{'knowledge':100000,'source':100000,'edges':500000}}}
record=measure_process([build['executable'],'check'],repo,b'{}\n',{'timeout':120})
record['validation']=validate_result(scenario,record)
record['timers']=[json.loads(line) for line in bytes.fromhex(record['stderr']['capture_hex']).decode().splitlines()]
record['build']=build;record['scope']='Separate instrumented inclusive wall-time scopes; nested times overlap; not CPU sampling or exact SQLite wait accounting'
write_json(evidence/'receipt.json',record)
assert record['validation']['valid']
print(record['duration_seconds'],record['validation'])
for row in sorted(record['timers'],key=lambda r:r['inclusive_nanoseconds'],reverse=True)[:7]:print(row)
