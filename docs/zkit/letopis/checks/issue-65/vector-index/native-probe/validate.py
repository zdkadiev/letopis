"""Validate saved native experiment evidence without running SQL or changing DBs."""
from pathlib import Path
import hashlib,json,statistics
ROOT=Path(__file__).resolve().parent
runs=json.loads((ROOT/'runs.json').read_text())
reference=(ROOT/'k100000-pair0-none.rows.tsv').read_bytes().splitlines(keepends=True)
# Given: the fixture has100,000 independently counted real vectors.
# When: the saved paired runs are validated for cardinality and full-channel-prefix equality.
# Then: each run returns exactly K stable row IDs and17-digit formatted distances in the same order.
assert len(reference)==100000
summary={'scope':'native SQLite/vec1 SQL only; includes result serialization, excludes product CLI refresh/provenance/filter/fusion/provider','query':'stored embedding rowid1;768 f32 dimensions','K':{}}
for k in [5,100,1000,100000]:
 expected=b''.join(reference[:k]);comparisons=[]
 for pair in range(3):
  for mode in ['none','flat']:
   data=(ROOT/f'k{k}-pair{pair}-{mode}.rows.tsv').read_bytes()
   assert data==expected,(k,pair,mode)
   comparisons.append({'pair':pair,'mode':mode,'equal_full_exact_prefix':True,'sha256':hashlib.sha256(data).hexdigest()})
 stats={}
 for mode in ['none','flat']:
  records=[x for x in runs if x['K']==k and x['mode']==mode]
  assert len(records)==3
  stats[mode]={'sqlite_cpu_seconds':[x['sqlite_timers'][0]['cpu_seconds'] for x in records],'sqlite_wall_seconds':[x['sqlite_timers'][0]['wall_seconds'] for x in records]}
  stats[mode]['median_cpu_seconds']=statistics.median(stats[mode]['sqlite_cpu_seconds']);stats[mode]['median_wall_seconds']=statistics.median(stats[mode]['sqlite_wall_seconds'])
 summary['K'][str(k)]={'samples':stats,'cpu_speedup':stats['none']['median_cpu_seconds']/stats['flat']['median_cpu_seconds'],'all_outputs_equal':True,'comparisons':comparisons}
# Given: SQLite SHA3 serialization contains ordered row IDs and complete vector BLOBs.
# When: the independent saved scans of both physical representations are compared.
# Then: all100,000 vectors have the same bytes and total size after rebuilding.
none=json.loads((ROOT/'vector-hash-none.receipt.json').read_text())['stdout']
flat=json.loads((ROOT/'vector-hash-flat.receipt.json').read_text())['stdout']
assert none==flat
assert '"rows":100000,"vector_bytes":307200000' in none
summary['ordered_vector_hash_and_size_equal']=True
(ROOT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print('saved outputs, full-vector hashes and cardinalities: PASS')
