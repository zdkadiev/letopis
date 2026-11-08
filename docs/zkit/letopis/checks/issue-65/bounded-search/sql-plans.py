from pathlib import Path
import re,json,struct,subprocess,hashlib
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;fixture=base/'repository';out=base/'sql-plans-v2';out.mkdir(exist_ok=False)
vector=json.loads((base/'oracle/query-vector.json').read_text())['values'];blob=struct.pack('<768f',*vector).hex()
source_files={'fts':root/'src/storage/cache.rs','vectors':root/'src/storage/vector.rs'}
prefixes={'fts':'select nodes.uri, nodes.id, bm25(text_index) as score','vectors':'select n.uri, k.search_hash, v.distance'}
receipt={}
for channel,path in source_files.items():
 text=path.read_text();sql=re.search('"('+re.escape(prefixes[channel])+r'.*?)"',text,re.S)[1]
 for limit in [5,100]:
  params=(f"insert into temp.sqlite_parameters(key,value) values('?1','\"case\" AND \"000000\"'),('?2','source'),('?3',{limit});" if channel=='fts' else f"insert into temp.sqlite_parameters(key,value) values('?1',X'{blob}'),('?2','{{K:100000}}'),('?3','source'),('?4',{limit});")
  pre=f".load {root}/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init\nattach '{fixture}/.letopis/vectors.db' as vec;\n.explain off\n.parameter init\n{params}\n"
  for mode,explain in [('plan','explain query plan '),('vm','explain ')]:
   run=pre+explain+sql+';\n';name=f'{limit}-{channel}-{mode}'
   (out/(name+'.sql')).write_text(run)
   result=subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',str(fixture/'.letopis/cache.db')],input=run,text=True,capture_output=True,check=True)
   (out/(name+'.json')).write_text(result.stdout);assert json.loads(result.stdout)
  receipt[f'{limit}-{channel}']={'source':str(path.relative_to(root)),'sql_sha256':hashlib.sha256(sql.encode()).hexdigest(),'bound':limit,'SQL_before_Rust_decoding':True,'native_complete_K':100000 if channel=='vectors' else None}
(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('8 actual-product SQL plan/bytecode captures saved')
