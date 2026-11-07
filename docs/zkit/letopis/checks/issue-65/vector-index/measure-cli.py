"""Serialized one-call observation on the owned full-vector fixture."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import time

root = Path.cwd()
base = root / '.artifacts/issue-65/vector-index-2026-11-07'
fixture = base / 'repository'
sys.path.insert(0, str(root / 'tests/performance'))
from measure import measure_process
from scenarios import get_environment, encode_json

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--source', type=Path, required=True)
parser.add_argument('--pin', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--label', required=True)
parser.add_argument('--model', choices=('none', 'flat'), required=True)
parser.add_argument('--conditioning', action='store_true')
args = parser.parse_args()
assert not args.output.exists()
args.output.mkdir(parents=True)
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
pin = json.loads(args.pin.read_text())
assert sha(args.binary) == pin['sha256']
for name, digest in pin['inputs'].items():
    assert sha(args.source / name) == digest, name

extension = root / '.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib'
sql = f""".load {extension} sqlite3_extension_init
select model from vec1cat where database='main' and name='vector_index';
"""
native = subprocess.run(['/opt/homebrew/opt/sqlite/bin/sqlite3', '-readonly', '-json',
                         str(fixture / '.letopis/vectors.db')], input=sql,
                        text=True, capture_output=True, check=True)
model = json.loads(json.loads(native.stdout)[0]['model'])
assert model == {'index': args.model, 'distance': 'cos'}, model
config = json.loads((fixture / '.letopis/config.json').read_text())
assert config['embedding_model']['enabled'] is True
def state():
    with sqlite3.connect((fixture / '.letopis/vectors.db').as_uri() + '?mode=ro', uri=True) as c:
        return c.execute('select (select count(*) from vector_keys), (select count(*) from embed_failures), (select config_identity from meta where id=1)').fetchone()
before = state()
assert before[0:2] == (100000, 0)
with sqlite3.connect((fixture / '.letopis/cache.db').as_uri() + '?mode=ro', uri=True) as c:
    known = {row[0] for row in c.execute("select uri from nodes where kind in ('vertex','metavertex')")}
    counts = c.execute("select (select count(*) from nodes where kind='source'), (select count(*) from edges)").fetchone()
assert len(known) == 100000 and counts == (100000, 500000)
request = {'query': 'case 000000', 'limit': 5}
manifest = {'label': args.label, 'conditioning': args.conditioning,
            'binary': str(args.binary), 'source': str(args.source), 'pin': pin,
            'repository': str(fixture), 'model': model, 'state': before,
            'request': request, 'config': config,
            'utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'scope': 'Fresh CLI process; direct-child wait4 excludes Ollama; shared host/cache uncontrolled. Conditioning excluded from comparison.'}
(args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
env = get_environment()
for name in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):
    env.pop(name, None)
env.update(NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
startup = measure_process([str(args.binary), '--help'], fixture, b'', {'timeout': 120, 'environment': env})
(args.output / 'startup.json').write_text(json.dumps(startup) + '\n')
assert startup['outcome'] == 'success'
result = measure_process([str(args.binary), 'search'], fixture, encode_json(request), {'timeout': 300, 'environment': env})
(args.output / 'raw.json').write_text(json.dumps(result) + '\n')
response = json.loads(bytes.fromhex(result['stdout']['capture_hex']))
assert result['exit_code'] == 0 and result['outcome'] == 'success' and response['ok'], response
assert not response.get('warnings')
items = response['result']
ids = {item['element_id'] for item in items}
expected = {
    'knowledge/part-0000.md#node-000000',
    'knowledge/part-0666.md#node-019999',
    'knowledge/part-0999.md#node-029999',
    'knowledge/part-3330.md#node-099900',
    'knowledge/part-0733.md#node-022000',
}
assert len(items) == len(ids) == 5 and ids == expected and ids <= known
assert all(not item['flags'] for item in items)
assert state() == before and sha(args.binary) == pin['sha256']
result['validation'] = {'valid': True, 'known_current_results': 5, 'literal_lexical_hit_retained': True,
                        'exact_five_id_set_matches_prior_frozen_baseline': True,
                        'unchanged_vector_state': True, 'native_model_before': model}
(args.output / 'measurement.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({'label': args.label, 'seconds': result['duration_seconds'], 'conditioning': args.conditioning,
                  'stdout_sha256': result['stdout']['sha256'], 'valid': True}), flush=True)
