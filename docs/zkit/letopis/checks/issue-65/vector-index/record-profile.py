"""Record a separate genuine full-query Samply profile after ordinary pairs."""
from pathlib import Path
import hashlib
import importlib.util
import json
import sys

root = Path.cwd()
base = root / '.artifacts/issue-65/vector-index-2026-11-07'
out = base / 'profile'
assert not out.exists()
out.mkdir()
pin = json.loads((base / 'product/after-build.json').read_text())
binary = Path(pin['executable'])
assert hashlib.sha256(binary.read_bytes()).hexdigest() == pin['sha256']
sys.path.insert(0, str(root / 'tests/performance'))
from measure import measure_process
from scenarios import get_environment, encode_json
spec = importlib.util.spec_from_file_location('letopis_profile', root / 'tests/performance/profile.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
env = get_environment()
for name in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):
    env.pop(name, None)
env.update(NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
command = ['/Users/zdkadiev/.cargo/bin/samply', 'record', '--save-only', '--rate', '1000',
    '--unstable-presymbolicate', '--output', str(out/'search.json.gz'), str(binary), 'search']
receipt = measure_process(command, base/'repository', encode_json({'query':'case 000000','limit':5}),
    {'timeout':300, 'environment':env})
receipt.update(argv=command, profile_only=True, binary_sha256=pin['sha256'],
    scope='Separate genuine Samply recording after ordinary comparison; wrapper wait4 is not CLI-only ordinary resources.')
(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert receipt['outcome']=='success' and receipt['exit_code']==0
ordinary = json.loads((base/'pairs/0-after/measurement.json').read_text())
assert receipt['stdout']['sha256']==ordinary['stdout']['sha256']
validation = module.validate_profile(out/'search.json.gz')
stacks = module.summarize_stacks(out/'search.json.gz',out/'search.json.syms.json')
(out/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
(out/'stacks.json').write_text(json.dumps(stacks,indent=2)+'\n')
assert hashlib.sha256(binary.read_bytes()).hexdigest()==pin['sha256']
print(json.dumps({'validation':validation,'observed_cpu_delta':stacks['observed_cpu_delta'],
                  'functions':stacks['functions'][:12]},indent=2),flush=True)
