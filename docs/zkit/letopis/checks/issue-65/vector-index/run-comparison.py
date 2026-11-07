"""Final serialized comparison; refuse to overwrite any previous evidence."""
from pathlib import Path
import hashlib
import json
import math
import statistics
import subprocess
import sys
import time
import urllib.request

root = Path.cwd()
base = root / '.artifacts/issue-65/vector-index-2026-11-07'
fixture = base / 'repository'
host_gate = json.loads((base / 'host-storage/manifest.json').read_text())
assert host_gate['status'] == 'PASS', 'finish actual-root storage verification before timing'
assert not (base / 'pairs').exists()
with (base / 'comparison-reservation.json').open('x') as stream:
    json.dump({'reserved_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
               'scope': 'Exclusive campaign reservation; failed attempts cannot overwrite evidence.'}, stream)
(base / 'mode-setups').mkdir()
before_pin = root / '.artifacts/issue-65/search-optimization-2026-11-06/after-build.json'
after_pin = base / 'product/after-build.json'
before = json.loads(before_pin.read_text())
after = json.loads(after_pin.read_text())
assert len(before['inputs']) == 106 and len(after['inputs']) == 107
assert 'src/storage/vector_index.rs' in after['inputs']
assert after['product_commit'] == '016eae8fa9705a209b01555a32877702b70cf74a'
with (base / 'comparison-manifest.json').open('x') as stream:
    json.dump({'before': before, 'after': after, 'n_per_side': 3,
               'order': 'before/after; after/before; before/after',
               'driver_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'measurement_driver_sha256': hashlib.sha256((base/'measure-cli.py').read_bytes()).hexdigest(),
               'scope': 'Fresh steady-state processes; full-vector copied fixture; setup/warmup/help outside timers'}, stream, indent=2)
variants = {
    'before': (before, before_pin, root / '.artifacts/issue-65/search-optimization-2026-11-06/combined/source', 'none'),
    'after': (after, after_pin, root, 'flat'),
}
for pin, pin_path, source, mode in variants.values():
    assert hashlib.sha256(Path(pin['executable']).read_bytes()).hexdigest() == pin['sha256']
    for name, digest in pin['inputs'].items():
        assert hashlib.sha256((source / name).read_bytes()).hexdigest() == digest, name
config = json.loads((fixture / '.letopis/config.json').read_text())['embedding_model']
request = urllib.request.Request(config['parameters']['base_url'] + '/embed',
    data=json.dumps({'model': config['model'], 'input': 'case 000000'}).encode(),
    headers={'Content-Type': 'application/json'}, method='POST')
started = time.monotonic()
response = json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=60))
assert len(response['embeddings']) == 1 and len(response['embeddings'][0]) == 768
assert all(math.isfinite(value) for value in response['embeddings'][0])
with (base / 'model-warmup.json').open('x') as stream:
    json.dump({'seconds': time.monotonic()-started,
    'dimensions': 768, 'model': config['model'],
    'scope': 'Query-only direct provider warmup outside timers; no vector database writes'}, stream)
extension = root / '.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib'
records = []
for pair in range(3):
    for side in (('before', 'after') if pair % 2 == 0 else ('after', 'before')):
        pin, pin_path, source, mode = variants[side]
        sql = f""".bail on
.load {extension} sqlite3_extension_init
select model from vec1cat where database='main' and name='vector_index';
insert into vector_index(cmd,arg) values('rebuild','{{index:"{mode}",distance:"cos"}}');
select model from vec1cat where database='main' and name='vector_index';
"""
        started = time.monotonic()
        command = ['/opt/homebrew/opt/sqlite/bin/sqlite3', '-json', str(fixture / '.letopis/vectors.db')]
        try:
            result = subprocess.run(command, input=sql, text=True,
                capture_output=True, check=False, timeout=120)
            setup = {'seconds': time.monotonic()-started, 'sql': sql,
                     'stdout': result.stdout, 'stderr': result.stderr,
                     'exit_code': result.returncode, 'outcome': 'success' if result.returncode == 0 else 'nonzero',
                     'scope': 'Owned fixture physical-mode rebuild outside timed CLI; unchanged vectors/model identity'}
        except subprocess.TimeoutExpired as error:
            setup = {'seconds': time.monotonic()-started, 'sql': sql,
                     'stdout': error.stdout.decode(errors='replace') if isinstance(error.stdout, bytes) else error.stdout,
                     'stderr': error.stderr.decode(errors='replace') if isinstance(error.stderr, bytes) else error.stderr,
                     'exit_code': None, 'outcome': 'timeout', 'timeout_seconds': 120}
            with (base / 'mode-setups' / f'{pair}-{side}.json').open('x') as stream:
                json.dump(setup, stream, indent=2)
            raise
        with (base / 'mode-setups' / f'{pair}-{side}.json').open('x') as stream:
            json.dump(setup, stream, indent=2)
        result.check_returncode()
        output = base / 'pairs' / f'{pair}-{side}'
        subprocess.run([sys.executable, '-B', str(base / 'measure-cli.py'),
            '--binary', pin['executable'], '--source', str(source), '--pin', str(pin_path),
            '--output', str(output), '--label', side, '--model', mode], cwd=root, check=True)
        record = json.loads((output / 'measurement.json').read_text())
        record.update(side=side, pair=pair)
        records.append(record)
assert len({record['stdout']['sha256'] for record in records}) == 1
summary = {side: {
    'seconds': [record['duration_seconds'] for record in records if record['side'] == side],
    'median_seconds': statistics.median(record['duration_seconds'] for record in records if record['side'] == side),
    'median_cpu_seconds': statistics.median(record['resources']['cpu_seconds']['value'] for record in records if record['side'] == side),
    'max_rss_bytes': max(record['resources']['peak_rss']['value'] for record in records if record['side'] == side),
    } for side in variants}
summary.update(n_per_side=3, valid_calls=len(records), output_identical=True,
    speedup=summary['before']['median_seconds']/summary['after']['median_seconds'],
    latency_reduction_percent=100*(1-summary['after']['median_seconds']/summary['before']['median_seconds']),
    scope='Steady-state full CLI on one copied synthetic100k real-provider corpus; mode setup/warmup/help/conditioning excluded; no intentional concurrent owned build/test/profile/drain; host and OS cache uncontrolled')
(base / 'comparison-summary.json').write_text(json.dumps(summary, indent=2)+'\n')
print(json.dumps(summary, indent=2), flush=True)
