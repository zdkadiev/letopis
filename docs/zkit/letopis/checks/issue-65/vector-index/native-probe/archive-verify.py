"""Verify archived evidence bytes and receipts without SQL or product execution."""
from pathlib import Path
import gzip
import hashlib
import json

ROOT = Path(__file__).resolve().parent
manifest = json.loads((ROOT / 'archive.json').read_text())
by_source = {entry['source_relative']: entry for entry in manifest['entries']}


def get_original_bytes(source_name):
    entry = by_source[source_name]
    stored = (ROOT / entry['archive_relative']).read_bytes()
    assert len(stored) == entry['archive_bytes'], source_name
    assert hashlib.sha256(stored).hexdigest() == entry['archive_sha256'], source_name
    if entry['compression'] == 'gzip':
        assert int.from_bytes(stored[4:8], 'little') == 0, source_name
        original = gzip.decompress(stored)
    else:
        original = stored
    assert len(original) == entry['uncompressed_bytes'], source_name
    assert hashlib.sha256(original).hexdigest() == entry['uncompressed_sha256'], source_name
    return original


for name in by_source:
    get_original_bytes(name)
for checked in manifest['output_receipt_validation']:
    receipt = json.loads(get_original_bytes(checked['receipt']))
    output = get_original_bytes(checked['output_source'])
    assert hashlib.sha256(output).hexdigest() == receipt['output_sha256']
    assert len(output.splitlines()) == receipt['rows']
    if checked['accepted_timing']:
        assert receipt['rows'] == receipt['K']

runs = json.loads(get_original_bytes('runs.json'))
assert len(runs) == 24
assert all(not run['name'].startswith('warmup') for run in runs)
reference = get_original_bytes('k100000-pair0-none.rows.tsv').splitlines(keepends=True)
assert len(reference) == 100000
for run in runs:
    assert get_original_bytes(run['name'] + '.rows.tsv') == b''.join(reference[:run['K']])
none = json.loads(get_original_bytes('vector-hash-none.receipt.json'))['stdout']
flat = json.loads(get_original_bytes('vector-hash-flat.receipt.json'))['stdout']
assert none == flat
assert '"rows":100000,"vector_bytes":307200000' in none
for entry in manifest.get('generated_files', []):
    data = (ROOT / entry['path']).read_bytes()
    assert len(data) == entry['bytes']
    assert hashlib.sha256(data).hexdigest() == entry['sha256']
print(f"Verified {len(by_source)} archived original files,27 output receipts,24 matched runs and full-vector hash equality.")
