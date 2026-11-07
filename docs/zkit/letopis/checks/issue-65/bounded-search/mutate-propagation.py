from pathlib import Path
import subprocess,os,json,hashlib
base=Path(__file__).parent;p=base/'source/src/service/search.rs';original=p.read_bytes();s=original.decode();receipts=[]
env=dict(os.environ,CARGO_TARGET_DIR='/Users/zdkadiev/Repos/letopis/target',CARGO_BUILD_JOBS='2')
for name,old,new in [
 ('fts-propagation','text_index_get_by_expression(&expression, limit)','text_index_get_by_expression(&expression, limit.max(MAX_SEARCH_LIMIT))'),
 ('vector-propagation','vector_index_get_by_vector(&identity, &vector, dimensions.get(), limit)','vector_index_get_by_vector(&identity, &vector, dimensions.get(), limit.max(MAX_SEARCH_LIMIT))'),
]:
 assert s.count(old)==1
 try:
  p.write_text(s.replace(old,new,1))
  command=['cargo','test','--locked','--offline','--lib','explicit_five_limits_both_hybrid_channels_before_observation']
  r=subprocess.run(command,cwd=base/'source',env=env,capture_output=True,text=True)
  (base/'mutations'/f'{name}.log').write_text(r.stdout+r.stderr)
  assert r.returncode!=0 and 'test result: FAILED' in r.stdout,(name,r.stdout,r.stderr)
  receipts.append({'name':name,'command':command,'exit':r.returncode,'caught':True,'source_sha256':hashlib.sha256(original).hexdigest()})
 finally:p.write_bytes(original)
 print(name,'caught and restored',flush=True)
 (base/'mutations/propagation-receipts.json').write_text(json.dumps(receipts,indent=2)+'\n')
