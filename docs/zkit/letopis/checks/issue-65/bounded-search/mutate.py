from pathlib import Path
import subprocess,os,json,hashlib
base=Path(__file__).parent;source=base/'source';out=base/'mutations';out.mkdir(exist_ok=False)
env=dict(os.environ,CARGO_TARGET_DIR='/Users/zdkadiev/Repos/letopis/target',CARGO_BUILD_JOBS='2')
cases=[
 ('fts-sql-cap','src/storage/cache.rs','limit ?3','limit (?3 * 0 + 1000000)',['--lib','--test','storage_cache','sql_']),
 ('vector-sql-cap','src/storage/vector.rs','limit ?4','limit (?4 * 0 + 1000000)',['--test','storage_vectors','nearest_limits_current_knowledge']),
 ('service-clamp','src/service/search.rs','let limit = request.limit.min(MAX_SEARCH_LIMIT);','let limit = request.limit.min(MAX_SEARCH_LIMIT.max(request.limit));',['--test','knowledge_search','service_caps_direct']),
 ('premature-native-k','src/storage/vector.rs','vector_index_get_by_vector(connection, &bytes, count, limit)','vector_index_get_by_vector(connection, &bytes, i64::try_from(limit.get()).map_err(map_invalid_value)?, limit)',['--test','storage_vectors','nearest_limits_current_knowledge']),
]
records=[]
for name,path,old,new,args in cases:
 p=source/path;original=p.read_bytes();text=original.decode();assert text.count(old)==1,(name,text.count(old))
 try:
  p.write_text(text.replace(old,new,1))
  run=subprocess.run(['cargo','test','--locked','--offline',*args],cwd=source,env=env,capture_output=True,text=True)
  (out/(name+'.log')).write_text(run.stdout+run.stderr)
  assert run.returncode!=0 and 'test result: FAILED' in run.stdout,(name,run.stdout,run.stderr)
  records.append({'name':name,'command':['cargo','test','--locked','--offline',*args],'exit':run.returncode,'caught':True,'source_sha256':hashlib.sha256(original).hexdigest()})
 finally:p.write_bytes(original)
 assert p.read_bytes()==original
 print(name,'caught and restored',flush=True)
 (out/'receipts.json').write_text(json.dumps(records,indent=2)+'\n')
