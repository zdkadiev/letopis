import hashlib
import json
import statistics
from collections import defaultdict
from pathlib import Path

root = Path(__file__).resolve().parent
samples = defaultdict(list)
for line in (root / 'native-probe.tsv').read_text().splitlines():
    columns = line.split('\t')
    if columns[0] != 'timing':
        continue
    _, query, cached, count, rows, fullscan, seconds = columns
    # Given: the independent fixture manifest contains 100,000 knowledge nodes and 500,000 edges.
    # When: each scoped query visits every known knowledge identity.
    # Then: both prepare strategies return the literal fixture totals with zero full-scan steps.
    assert int(count) == 100000
    assert int(rows) == (500000 if query == 'edges' else 100000)
    assert int(fullscan) == 0
    samples[query, cached].append(float(seconds))
summary = {
    'scope': 'SQL-only C probe of copied cache, pinned SQLite 3.53.2 amalgamation; excludes SeaQuery, Rust decoding, filesystem, parsing, provenance, embeddings and full search',
    'engine_flags': ['-O2', '-DSQLITE_ENABLE_FTS5', '-DSQLITE_THREADSAFE=1'],
    'build_parity_limit': 'Same pinned SQLite source as libsqlite3-sys 0.38.2; simplified compile flags, not the exact product binary',
    'samples_seconds': {f'{query}_cached_{cached}': values for (query, cached), values in samples.items()},
    'medians_seconds': {f'{query}_cached_{cached}': statistics.median(values) for (query, cached), values in samples.items()},
    'input_sha256': {name: hashlib.file_digest((root / name).open('rb'), 'sha256').hexdigest() for name in ['native-probe.c', 'native-probe', 'cache-copy.db']},
}
(root / 'native-probe-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary['medians_seconds'], indent=2))
