import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,'tests/transport')
from run import sources
root=Path('docs/zkit/letopis/checks/TK-6')
for platform,count in [('macos-arm64',10),('linux-arm64',11)]:
 path=root/platform/'manifest.json'
 r=json.loads(path.read_text())
 assert r['result']=='PASS'
 assert r['source_sha256']==r['source_after_sha256']==sources()
 assert len(r['prior_proofs'])==10 and all(not x['mismatches'] and x['result']=='PASS' for x in r['prior_proofs'])
 for relative,expected in r['evidence_sha256'].items(): assert hashlib.sha256((path.parent/relative).read_bytes()).hexdigest()==expected,relative
 controls=json.loads((path.parent/'negative-controls/manifest.json').read_text())
 assert controls['result']=='PASS' and len(controls['controls'])==count
 assert all(x['result']=='DETECTED' and x['build_exit']==0 and x['test_exit']!=0 and x['original_sha256']==x['restored_sha256'] for x in controls['controls'])
 assert controls['source_before_sha256']==controls['source_after_sha256']==sources()
 print(platform,'PASS; current sources, receipt hashes, committed prior proofs and',count,'compiling controls verified')
assert json.loads((root/'system-tls-macos-arm64/manifest.json').read_text())['result']=='PASS'
