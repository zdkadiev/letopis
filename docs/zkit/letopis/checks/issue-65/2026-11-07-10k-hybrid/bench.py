from pathlib import Path
import sys,os,json,hashlib,sqlite3,statistics
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;repo=base/'fixture/repository'
sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
bins={'before':root/'.artifacts/issue-65/vector-index-2026-11-07/product/letopis','after':root/'.artifacts/issue-65/bounded-search-2026-11-07/product/letopis'}
hashes={k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in bins.items()}
assert hashes=={'before':'ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9','after':'117bee3d746af4946c0a3a52f2aa6e8bd972e06ba25d2e982b1fe27a73af53ba'}
with sqlite3.connect(repo/'.letopis/vectors.db') as c: assert c.execute('select count(*) from vector_keys').fetchone()[0]==10000 and c.execute('select count(*) from embed_failures').fetchone()[0]==0
with sqlite3.connect(repo/'.letopis/cache.db') as c: assert c.execute("select count(*) from nodes where kind <> 'source'").fetchone()[0]==10000
env=dict(os.environ)
for k in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(k,None)
env.update(NO_PROXY='localhost,127.0.0.1',no_proxy='localhost,127.0.0.1')
records=[]
def run(side,limit,name):
 r=measure_process([str(bins[side]),'search'],repo,json.dumps({'query':'case 000000','limit':limit}).encode(),{'timeout':120,'environment':env,'capture_limit':1048576})
 (base/(name+'.json')).write_text(json.dumps(r,indent=2)+'\n')
 assert r['outcome']=='success' and r['exit_code']==0,r
 data=json.loads(bytes.fromhex(r['stdout']['capture_hex']));assert data['ok'],data
 suffix='before' if side=='before' else 'expected';expected=json.loads((base/'oracle'/f'{limit}-{suffix}.json').read_text())['expected_ids']
 assert [x['element_id'] for x in data['result']]==expected
 assert all(not x['flags'] for x in data['result'])
 record={'side':side,'limit':limit,'seconds':r['duration_seconds'],'cpu_seconds':r['resources']['cpu_seconds']['value'],'rss_bytes':r['resources']['peak_rss']['value'],'sha256':r['stdout']['sha256']}
 print(name,json.dumps(record),flush=True);return record
run('before',5,'conditioning-before');run('after',5,'conditioning-after')
for limit in (5,100):
 for pair in range(3):
  for side in (('before','after') if pair%2==0 else ('after','before')):records.append(run(side,limit,f'{limit}-{pair}-{side}'))
summary={}
for limit in (5,100):
 result={}
 for side in ('before','after'):
  rows=[r for r in records if r['limit']==limit and r['side']==side]
  result[side]={'samples':[r['seconds'] for r in rows],'median':statistics.median(r['seconds'] for r in rows),'max_rss_bytes':max(r['rss_bytes'] for r in rows)}
 result['speedup']=result['before']['median']/result['after']['median'];summary[limit]=result
(base/'summary.json').write_text(json.dumps({'binaries':hashes,'corpus':10000,'vectors':10000,'query':'case 000000','records':records,'summary':summary},indent=2)+'\n')
print(json.dumps(summary,indent=2),flush=True)
