"""Genuine Samply, recorded separately after all ordinary measurements."""
from pathlib import Path
import json,hashlib,importlib.util,sys,os,subprocess,gzip,sqlite3,re
root=Path('/Users/zdkadiev/Repos/letopis');base=Path(__file__).parent;out=base/'profile';out.mkdir(exist_ok=False)
assert (base/'comparison-summary.json').is_file()
pin=json.loads((base/'product/pin.json').read_text());binary=base/'product/letopis'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(binary)==pin['sha256']
for path,digest in pin['inputs'].items():assert sha(root/path)==digest,path
sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
spec=importlib.util.spec_from_file_location('letopis_profile',root/'tests/performance/profile.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
env=dict(os.environ)
for name in ['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy']:env.pop(name,None)
env.update(NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
command=['/Users/zdkadiev/.cargo/bin/samply','record','--save-only','--rate','1000','--unstable-presymbolicate','--output',str(out/'search.json.gz'),str(binary),'search']
receipt=measure_process(command,base/'repository',b'{"query":"case 000000","limit":100}',{'timeout':300,'environment':env,'capture_limit':1048576})
receipt.update(argv=command,scope='Separate genuine Samply after ordinary comparisons; wrapper wait4 includes profiler and is not ordinary CLI CPU/RSS.',binary_sha256=pin['sha256'])
(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert receipt['outcome']=='success' and receipt['exit_code']==0,receipt
ordinary=json.loads((base/'pairs/100-0-after/measurement.json').read_text());assert receipt['stdout']['sha256']==ordinary['stdout']['sha256']
validation=module.validate_profile(out/'search.json.gz');stacks=module.summarize_stacks(out/'search.json.gz',out/'search.json.syms.json')
(out/'validation.json').write_text(json.dumps(validation,indent=2)+'\n');(out/'stacks.json').write_text(json.dumps(stacks,indent=2)+'\n')
with gzip.open(out/'search.json.gz','rt') as f:profile=json.load(f)
libs=[lib for lib in profile['libs'] if lib['name']=='letopis'];assert len(libs)==1
uuid_text=subprocess.check_output(['dwarfdump','--uuid',str(binary)],text=True)
uuid=re.search(r'UUID: ([0-9A-F-]+)',uuid_text)[1].replace('-','').upper()
assert libs[0]['codeId'].upper()==uuid and libs[0]['breakpadId'].upper()==uuid+'0'
symbols=json.loads((out/'search.json.syms.json').read_text())
owned_symbols=[data for data in symbols['data'] if data.get('debug_name')=='letopis'];assert len(owned_symbols)==1
assert owned_symbols[0]['code_id'].upper()==uuid and owned_symbols[0]['debug_id'].replace('-','').upper()==uuid
assert Path(libs[0]['path']).resolve()==binary.resolve()
(out/'identity.json').write_text(json.dumps({'profile_library':libs[0],'binary_sha256':pin['sha256'],'dwarfdump_uuid':uuid_text,'profile_helper_sha256':sha(root/'tests/performance/profile.py'),'samply_version':subprocess.check_output(['/Users/zdkadiev/.cargo/bin/samply','--version'],text=True).strip()},indent=2)+'\n')
assert sha(binary)==pin['sha256']
print(json.dumps({'validation':validation,'observed_cpu_delta':stacks['observed_cpu_delta'],'unit':stacks['cpu_delta_unit'],'functions':stacks['functions'][:15]},indent=2),flush=True)
