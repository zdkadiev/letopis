from pathlib import Path
import sys,json,os
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent
sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
out=base/'conditioning.json';assert not out.exists()
env=dict(os.environ)
for n in ['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy']:env.pop(n,None)
env.update(NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
r=measure_process([str(root/'.artifacts/issue-65/vector-index-2026-11-07/product/letopis'),'search'],base/'repository',b'{"query":"case 000000","limit":5}',{'timeout':600,'environment':env})
out.write_text(json.dumps(r,indent=2)+'\n');assert r['outcome']=='success',r
v=json.loads(bytes.fromhex(r['stdout']['capture_hex']));assert v['ok'] and len(v['result'])==5
print(json.dumps({'conditioning_seconds':r['duration_seconds'],'excluded':True,'ids':[x['element_id'] for x in v['result']]}),flush=True)
