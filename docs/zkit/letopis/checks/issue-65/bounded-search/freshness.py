"""Independent byte/range oracle for the selected synthetic knowledge and bindings."""
from pathlib import Path
import json,hashlib,sqlite3,subprocess,tarfile,io,re
base=Path(__file__).parent;root=Path('/Users/zdkadiev/Repos/letopis');fixture=(base/'repository').resolve()
manifest=root/'.artifacts/issue-65/2026-11-06-100k-fts600/fixture/corpus.json';canonical=json.loads(manifest.read_text());out=base/'freshness.json';assert not out.exists()
sha=lambda data:hashlib.sha256(data).hexdigest()
for path,digest in canonical['input_sha256'].items():
 p=fixture/path;assert not p.is_symlink() and sha(p.read_bytes())==digest,path
ids=set()
for limit in [5,100]:
 for suffix in ['expected','before']:ids.update(json.loads((base/'oracle'/f'{limit}-{suffix}.json').read_text())['expected_ids'])
for channel in ['fts','vectors']:ids.update(x['id'] for x in json.loads((base/'oracle'/f'100-{channel}.json').read_text()))
basis=canonical['source_tree']
archive=subprocess.check_output(['git','archive','--format=tar',basis,'sources'],cwd=fixture)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
 historical={member.name:tar.extractfile(member).read() for member in tar if member.isfile()}
assert len(historical)==1000
for path,data in historical.items():assert data==(fixture/path).read_bytes(),path
records=[]
with sqlite3.connect((fixture/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True) as c:
 for id in sorted(ids):
  row=c.execute('select id,hash,line_start,line_end,source_snapshot from nodes where uri=?',[id]).fetchone();assert row is not None
  key,digest,start,end,snapshot=row;assert snapshot==basis
  path,anchor=id.split('#');lines=(fixture/path).read_bytes().splitlines(keepends=True);raw=b''.join(lines[start-1:end]);assert sha(raw)==digest,id
  assert lines[start-1].decode().rstrip().endswith('{#'+anchor+'}'),id
  bindings=c.execute("select n.uri from edges e join nodes n on n.id=e.target_id where e.source_id=? and n.kind='source' order by n.uri",[key]).fetchall();assert len(bindings)==1,id
  for (uri,) in bindings:
   match=re.fullmatch(r'(.+)#L([0-9]+)-L([0-9]+)',uri);assert match,uri
   path,first,last=match[1],int(match[2]),int(match[3]);live=(fixture/path).read_bytes().splitlines(keepends=True);old=historical[path].splitlines(keepends=True)
   assert 1<=first<=last<=len(live) and last<=len(old),uri
   current=b''.join(live[first-1:last]);previous=b''.join(old[first-1:last]);assert current==previous
   records.append({'knowledge':id,'knowledge_hash':digest,'source':uri,'source_sha256':sha(current),'source_snapshot':snapshot})
out.write_text(json.dumps({'scope':'Independent canonical file pins, cached knowledge range SHA-256/anchor and source binding lookup, plus exact Git-basis/current source byte equality. No product parser/provenance/hash helper used. Valid for this unchanged owned synthetic fixture, not a concurrent-edit guarantee.','canonical_manifest_sha256':sha(manifest.read_bytes()),'canonical_files':len(canonical['input_sha256']),'historical_source_files':len(historical),'verified_knowledge':len(ids),'verified_bindings':len(records),'records':records},indent=2)+'\n')
print(len(ids),'knowledge and',len(records),'source bindings verified')
