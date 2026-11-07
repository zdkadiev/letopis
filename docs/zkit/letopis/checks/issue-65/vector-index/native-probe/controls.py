from pathlib import Path
import json,subprocess,time
ROOT=Path(__file__).resolve().parent
CLI='/opt/homebrew/opt/sqlite/bin/sqlite3'
EXT='/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib'
records=[]
def run(name,db,sql,readonly=False):
 script=f'.bail on\n.load {EXT} sqlite3_extension_init\n.mode json\n'+sql+'\n'
 (ROOT/f'{name}.sql').write_text(script)
 start=time.monotonic();p=subprocess.run([CLI]+(['-readonly'] if readonly else [])+[str(db)],input=script.encode(),capture_output=True,timeout=90)
 r={'name':name,'exit_code':p.returncode,'wall_seconds':time.monotonic()-start,'stdout':p.stdout.decode(),'stderr':p.stderr.decode()}
 (ROOT/f'{name}.receipt.json').write_text(json.dumps(r,indent=2)+'\n');records.append(r);print(name,r,flush=True);return r
for mode in ['none','flat']:
 run(f'vector-hash-{mode}',ROOT/f'{mode}.db',"select hex(sha3_query('select rowid, vector from vector_index order by rowid',256)) as ordered_vector_sha3_256;\nselect count(*) as rows,sum(length(vector)) as vector_bytes from vector_index;",True)
query="select rowid,printf('%!.17g',distance) as distance from vector_index(x'0000803f00000000','{K:3}') order by distance,rowid;"
for case,blob in [('short','000080'),('nan','0000c07f0000803f'),('infinity','0000807f0000803f')]:
 db=ROOT/f'control-{case}.db'
 run(f'control-{case}-setup',db,"create virtual table vector_index using vec1(vector);\ninsert into vector_index(cmd,arg) values('rebuild','{index:\"none\",distance:\"cos\"}');\ninsert into vector_index(rowid,vector) values(1,x'0000803f00000000'),(2,x'000000000000803f'),(3,x'0000803f0000803f');\n"+f"update vector_index_base set vector=x'{blob}' where id=2;")
 run(f'control-{case}-before-query',db,query,True)
 run(f'control-{case}-before-read',db,"select rowid,hex(vector) as bytes from vector_index order by rowid;",True)
 run(f'control-{case}-migration',db,"insert into vector_index(cmd,arg) values('rebuild','{index:\"flat\",distance:\"cos\"}');")
 run(f'control-{case}-after-query',db,query,True)
 run(f'control-{case}-after-read',db,"select rowid,hex(vector) as bytes from vector_index order by rowid;\nselect hex(val) as model from vector_index_model;",True)
(ROOT/'control-runs.json').write_text(json.dumps(records,indent=2)+'\n')
