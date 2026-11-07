from pathlib import Path
import subprocess,json
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;out=base/'vector-preservation';out.mkdir(exist_ok=False)
expected='4ED319020B3ADFE2FD6FC375EE308B6266F2D9E74689A6DE1EF7E22488218EE4';records={}
for label,fixture in [('original',root/'.artifacts/issue-65/vector-index-2026-11-07/repository'),('owned',base/'repository')]:
 database=fixture/'.letopis/vectors.db';assert not Path(str(database)+'-wal').exists(),database
 pre=f'.load {root}/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init\n'
 uri=database.resolve().as_uri()+'?mode=ro&immutable=1'
 queries={'counts':'select count(*) as native_vectors, sum(length(vector)) as vector_bytes from vector_index;',
          'payload':"select hex(sha3_query('select rowid, vector from vector_index order by rowid')) as payload_sha3;"}
 record={}
 for kind,sql in queries.items():
  run=subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',uri],input=pre+sql,text=True,capture_output=True,check=True)
  (out/f'{label}-{kind}.json').write_text(run.stdout);(out/f'{label}-{kind}.sql').write_text(pre+sql+'\n');record.update(json.loads(run.stdout)[0])
 assert record=={'native_vectors':100000,'vector_bytes':307200000,'payload_sha3':expected},record
 records[label]=record
(out/'summary.json').write_text(json.dumps(records,indent=2)+'\n');print(records)
