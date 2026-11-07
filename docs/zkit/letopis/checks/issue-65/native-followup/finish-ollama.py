from pathlib import Path
import sys,time,json,subprocess,os
root=Path.cwd();p=root/'.artifacts/issue-65/2026-11-06-native-followup';e=p/'ollama-100k';repo=e/'repository'
sys.path.insert(0,str(root/'tests/performance'))
from scenarios import write_json,encode_json,get_environment,hash_file
from measure import measure_process
from profile import validate_profile,summarize_stacks
print('Waiting for successful complete same-config drain',flush=True)
while not (e/'complete-coverage.json').exists():time.sleep(5)
r=json.loads((e/'resume-drain.json').read_text());coverage=json.loads((e/'complete-coverage.json').read_text())
assert r['outcome']=='success' and coverage['vectors']==100000 and coverage['attempt_rows']==0,(r['outcome'],coverage)
print('Population complete; running independent native coverage oracle',flush=True)
with (p/'coverage.sql').open('rb') as src,(e/'native-complete-coverage.json').open('xb') as out:
 subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3','-readonly','-json',str(repo/'.letopis/vectors.db')],stdin=src,stdout=out,check=True)
subprocess.run([sys.executable,'-B',str(p/'full-ollama-reads.py')],cwd=root,check=True)
print('Full ordinary reads complete; recording a native mixed-query profile',flush=True)
build=json.loads((p/'after-build.json').read_text());assert hash_file(Path(build['executable']))==build['sha256']
env=get_environment()
for key in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(key,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
profile=e/'full-native-search.json.gz';assert not profile.exists()
argv=['/Users/zdkadiev/.cargo/bin/samply','record','--save-only','--rate','1000','--unstable-presymbolicate','--output',str(profile),build['executable'],'search']
captured=measure_process(argv,repo,encode_json({'query':'case 000000','limit':5}),{'timeout':600,'environment':env})
captured['profile_only']=True;write_json(e/'full-profile-receipt.json',captured)
assert captured['outcome']=='success',captured['outcome']
response=json.loads(bytes.fromhex(captured['stdout']['capture_hex']));items=response['result'];ids={x['element_id'] for x in items}
assert response['ok'] and not response.get('warnings') and len(items)==len(ids)==5 and 'knowledge/part-0000.md#node-000000' in ids and all(not x['flags'] for x in items),response
write_json(e/'full-native-validation.json',validate_profile(profile))
write_json(e/'full-native-stacks.json',summarize_stacks(profile,e/'full-native-search.json.syms.json'))
write_json(e/'completion.json',{'outcome':'complete','vectors':100000,'full_mixed_queries':3,'mixed_query_oracle':'one FTS match plus four vector-only known knowledge URIs','new_release_check_status_valid':True,'native_full_query_profile_valid':True})
print('Full Ollama campaign complete',flush=True)
