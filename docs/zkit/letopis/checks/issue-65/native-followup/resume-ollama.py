from pathlib import Path
import sys,json,sqlite3,os,time
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from scenarios import get_environment,encode_json,write_json,hash_file
from measure import measure_process
from contention import embed_lease
p=root/'.artifacts/issue-65/2026-11-06-native-followup';evidence=p/'ollama-100k';repo=evidence/'repository'
first=json.loads((evidence/'drain.json').read_text());assert first['outcome']=='nonzero' and first['exit_code']==-9
assert (evidence/'controlled-checkpoint.json').is_file()
build=json.loads((p/'before-build.json').read_text());assert hash_file(Path(build['executable']))==build['sha256']
config=json.loads((repo/'.letopis/config.json').read_text());assert config==json.loads((evidence/'manifest.json').read_text())['config']
with embed_lease(repo):
 c=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
 before={'vectors':c.execute('select count(*) from vector_keys').fetchone()[0],'attempt_rows':c.execute('select count(*) from embed_failures').fetchone()[0],'identity':c.execute('select config_identity from meta where id=1').fetchone()[0]};c.close()
 assert 0<before['vectors']<100000 and before['attempt_rows']==0,before
write_json(evidence/'resume-manifest.json',{'build_sha256':build['sha256'],'config':config,'before':before,'timeout_seconds':7200,'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'scope':'continuation of controlled partial drain; same corpus, identity and attempt budget, no external DB writes; shared host'})
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
r=measure_process([build['executable'],'embed','--drain'],repo,encode_json({}),{'timeout':7200,'environment':env})
write_json(evidence/'resume-drain.json',r);print('Resume',r['outcome'],r['duration_seconds'],flush=True)
c=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
after={'vectors':c.execute('select count(*) from vector_keys').fetchone()[0],'attempt_rows':c.execute('select count(*) from embed_failures').fetchone()[0],'identity':c.execute('select config_identity from meta where id=1').fetchone()[0]};c.close()
write_json(evidence/'complete-coverage.json',after);print(after,flush=True)
assert r['outcome']=='success' and json.loads(bytes.fromhex(r['stdout']['capture_hex']))['ok']
assert after==dict(before,vectors=100000)
