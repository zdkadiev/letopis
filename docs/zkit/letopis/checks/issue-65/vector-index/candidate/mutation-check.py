from pathlib import Path
import json,subprocess,os,hashlib
source=Path('src/storage/vector_index.rs');original=source.read_text()
mutants=[
 ('skip-migration','if flat || query_only || connection.is_readonly("main").map_err(map_sqlite)? {','if true || flat || query_only || connection.is_readonly("main").map_err(map_sqlite)? {','exact_flat_bootstrap_preserves_populated_state_and_is_idempotent'),
 ('skip-writer-model-recheck','if !is_exact_flat_index(&transaction)? {\n            verify_legacy_vectors','if true {\n            verify_legacy_vectors','exact_flat_competing_opener_rechecks_the_committed_model'),
 ('skip-original-vector-validation','verify_legacy_vectors(&transaction)?;','if false { verify_legacy_vectors(&transaction)?; }','exact_flat_bootstrap_does_not_repair_nonfinite_or_malformed_old_vectors'),
 ('accept-unknown-model','flat.ok_or_else(|| {','flat.or(Some(false)).ok_or_else(|| {','exact_flat_bootstrap_rejects_a_corrupt_distance_hidden_by_native_catalog'),
 ('accept-bad-version-bound','if model_version <= 0 || model_version == i64::MAX {','if false && (model_version <= 0 || model_version == i64::MAX) {','exact_flat_upgrade_does_not_repair_a_corrupt_native_model_version'),
 ('write-query-only','if flat || query_only || connection.is_readonly("main").map_err(map_sqlite)? {','if flat || (query_only && false) || connection.is_readonly("main").map_err(map_sqlite)? {','exact_flat_query_only_bootstrap_preserves_legacy_observation'),
]
env=os.environ|{'CARGO_TARGET_DIR':'../target-build','CARGO_BUILD_JOBS':'2'}
results=[]
try:
 for name,old,new,test in mutants:
  assert original.count(old)==1,(name,original.count(old))
  source.write_text(original.replace(old,new))
  result=subprocess.run(['cargo','test','--locked','--offline','--lib',f'storage::vector_index::tests::{test}'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  Path(f'../mutant-{name}.log').write_text(result.stdout)
  caught=result.returncode==101 and 'test result: FAILED.' in result.stdout
  results.append({'name':name,'test':test,'exit':result.returncode,'caught':caught})
  Path('../mutations.json').write_text(json.dumps(results,indent=2)+'\n')
  print(name,'CAUGHT' if caught else 'NOT CAUGHT',flush=True)
  source.write_text(original)
finally: source.write_text(original)
assert all(x['caught'] for x in results)
print('Restored SHA256',hashlib.sha256(source.read_bytes()).hexdigest(),flush=True)
