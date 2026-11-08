from pathlib import Path
import sys,json,os,statistics,sqlite3
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import build_request,build_inspection_oracle,write_json,encode_json,hash_file,FIRST_ID
from report import validate_result
from measure import measure_process
base=root/'.artifacts/issue-65';evidence=base/'2026-11-06-100k-inspect-pairs'
evidence.mkdir()
prior=base/'2026-11-06-100k-direct';assert json.loads((prior/'completion.json').read_text())['outcome']=='complete'
repo=Path(json.loads((prior/'manifest.json').read_text())['repository'])
builds={'before':json.loads((base/'2026-11-05-smoke-before/build/build.json').read_text()),'after':json.loads((base/'2026-11-05-optimized-build/build.json').read_text())}
changed={p for p,digest in builds['before']['inputs'].items() if builds['after']['inputs'].get(p)!=digest}
assert changed=={'src/git/cleanup.rs','src/git/durability.rs','src/git/objects.rs'},changed
for build in builds.values():assert hash_file(Path(build['executable']))==build['sha256']
scenario={'repository':repo,'expected':{'command':'inspect','id':FIRST_ID,'counts':{'knowledge':100000,'source':100000,'edges':500000},'inspection':build_inspection_oracle(repo,FIRST_ID)}}
write_json(evidence/'manifest.json',{'builds':builds,'repository':str(repo),'repetitions':3,'conditioning':0,'cache_state':'Initialized and previously captured synthetic corpus; OS page cache uncontrolled; fresh CLI process per call','comparison_state':'Same quiescent corpus; inspect writes idempotent Git objects/refs; projection revision and returned tree must remain unchanged','changed_product_inputs':sorted(changed)})
def get_revision():
 connection=sqlite3.connect((repo/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True)
 try:return connection.execute('select revision from project where id=1').fetchone()[0]
 finally:connection.close()
revision=get_revision()
records=[];snapshot=None
with (evidence/'samples.jsonl').open('x') as stream:
 for pair in range(3):
  for side in (('before','after') if pair%2==0 else ('after','before')):
   record=measure_process([builds[side]['executable'],'inspect'],repo,encode_json({'element_ids':[FIRST_ID]}),{'timeout':120})
   record.update(pair=pair,side=side,validation=validate_result(scenario,record),load_average=os.getloadavg())
   stream.write(json.dumps(record)+'\n');stream.flush();os.fsync(stream.fileno());records.append(record)
   assert record['validation']['valid'],record
   assert get_revision()==revision
   observed=record['validation']['observed']['snapshot']
   if snapshot is None:snapshot=observed
   assert snapshot==observed
   print(side,pair,round(record['duration_seconds'],4),flush=True)
summary={side:{'n':3,'median':statistics.median(x['duration_seconds'] for x in records if x['side']==side),'min':min(x['duration_seconds'] for x in records if x['side']==side),'max':max(x['duration_seconds'] for x in records if x['side']==side)} for side in builds}
summary['speedup']=summary['before']['median']/summary['after']['median'];summary['reduction_percent']=100*(1-summary['after']['median']/summary['before']['median']);summary['snapshot']=snapshot
write_json(evidence/'summary.json',summary);print(summary)
