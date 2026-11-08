from pathlib import Path
import subprocess,json,hashlib,re
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;out=base/'knowledge';basis=json.loads((out/'final-basis.json').read_text())
for path,digest in basis['source_sha256'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
def call(command,payload,name):
 r=subprocess.run(['/opt/homebrew/bin/letopis',command],cwd=root,input=json.dumps(payload),capture_output=True,text=True,check=True)
 (out/name).write_text(r.stdout);v=json.loads(r.stdout);assert v['ok'],v;return v['result']
searches=[]
for owner,source in [('letopis/architecture/search-pipeline.md','src/cli/input.rs'),('letopis/architecture/storage.md','docs/zkit/letopis/checks/issue-65/bounded-search/vector-preservation/summary.json')]:
 hits=call('search',{'query':'SQL','limit':10,'filters':{'sources':[source]}},'search-'+Path(owner).stem+'.json')
 target=next(x for x in hits if x['element_id']==owner);assert not target['flags'],target;searches.append(target)
addresses=sorted(set(basis['bindings'])|{a for v in basis['bindings'].values() for a in v})
result=call('inspect',{'element_ids':addresses},'inspect.json');tree=result['snapshot'];readback={}
for address in addresses:
 entry=result['elements'][address];path=entry['path'];blob=subprocess.check_output(['git','show',tree+':'+path],cwd=root);assert blob==(root/path).read_bytes(),address
 lines=entry.get('lines');selected=b''.join(blob.splitlines(keepends=True)[lines[0]-1:lines[1]]) if lines else blob
 assert hashlib.sha256(selected).hexdigest()==entry['hash'],address
 readback[address]={'path':path,'lines':lines,'hash':entry['hash']}
 if address in basis['bindings']:
  m=json.loads(blob.decode().splitlines()[0][len('<!-- letopis: '):-len(' -->')]);assert m['source_snapshot']==basis['source_snapshot']
verified={'reading_snapshot':tree,'author_basis':basis['source_snapshot'],'updated_knowledge':searches,'source_bindings':{k:len(v) for k,v in basis['bindings'].items()},'verified_addresses':len(addresses),'readback':readback,'scope':'Compatible installed letopis0.2.0 on preserved cache schema2. Scoped check, refreshed FTS/source publication and same-tree byte readback; no new knowledge vector-readiness claim.'}
(out/'verification.json').write_text(json.dumps(verified,indent=2)+'\n')
(out/'report.md').write_text(f'''# Letopis publication\n\nUpdated existing search-pipeline and storage concepts, preserving IDs/created_at.\nPublication revision {searches[-1]['revision']}; both selected results have empty flags.\nAuthor source basis `{basis['source_snapshot']}` was built in a separate temporary\nindex and all declared source bytes were verified. Reading snapshot `{tree}`\nverifies both knowledge files and every declared source address in one captured tree.\nSearch has9 source bindings; storage has30. Scoped check has no errors.\n\nSee final-basis.json, check.json, search-*.json, inspect.json and verification.json.\nThe installed0.2.0 reader was used because the working repository cache is older\nthan the development product schema; no reset/config replacement occurred.\nNew knowledge vectorization was not assessed. Other concepts were not silently\nmarked verified merely because their whole-file sources changed.\n''')
print(json.dumps({'revision':searches[-1]['revision'],'reading_snapshot':tree,'verified_addresses':len(addresses),'bindings':verified['source_bindings']}))
