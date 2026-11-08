from pathlib import Path
import os,subprocess,json
p=Path.cwd()/'.artifacts/issue-65/2026-11-06-native-followup'; source=p/'source'; git=source/'src/git'
backup=p/'green-src';backup.mkdir()
for name in ('objects.rs','reader.rs','validation.rs'):(backup/name).write_bytes((git/name).read_bytes())
original={name:(git/name).read_text() for name in ('reader.rs','refs.rs')}
mutants=[
 ('omit-memo','reader.rs','if !self.validated_trees.contains(oid) {','if true {','tree_reads_reuse_structure_only_within_the_owned_reader'),
 ('ignore-content-identity','reader.rs','if hash.finish()? != oid {','if hash.finish()? != oid && false {','memoized_tree_reads_reject_replacements_and_recover_after_restoration'),
 ('cache-invalid-structure','reader.rs','''            tree_bytes_validate(&bytes, self.oid_len)?;
            if self.validated_trees.len() < MAX_VALIDATED_TREES {
                self.validated_trees.insert(oid.to_owned());
            }''','''            if self.validated_trees.len() < MAX_VALIDATED_TREES {
                self.validated_trees.insert(oid.to_owned());
            }
            tree_bytes_validate(&bytes, self.oid_len)?;''','invalid_tree_bodies_are_validated_on_every_demand'),
 ('unbounded-memo','reader.rs','if self.validated_trees.len() < MAX_VALIDATED_TREES {','if self.validated_trees.len() <= MAX_VALIDATED_TREES {','tree_validation_memo_overflow_revalidates_without_rejecting_valid_objects'),
 ('omit-full-audit-structure','refs.rs','tree_bytes_validate(&bytes, git.oid_len)?;','if bytes.is_empty() { tree_bytes_validate(&bytes, git.oid_len)?; }','full_audits_validate_tree_structure_despite_addressed_read_memoization'),
]
env=dict(os.environ,CARGO_TARGET_DIR=str(p/'test-target'),CARGO_INCREMENTAL='0')
receipts=[]
try:
 for name,file,old,new,test in mutants:
  assert original[file].count(old)==1,(name,original[file].count(old))
  (git/file).write_text(original[file].replace(old,new,1))
  command=['cargo','test','--manifest-path',str(source/'Cargo.toml'),'--locked','--offline','--lib','git::objects::tests::'+test,'--','--exact']
  result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,env=env)
  text=result.stdout.decode(errors='replace');(p/(name+'.log')).write_text(text)
  receipt={'name':name,'test':test,'exit_code':result.returncode,'detected':result.returncode==101 and 'test result: FAILED' in text}
  receipts.append(receipt);(p/'mutations.json').write_text(json.dumps(receipts,indent=2)+'\n')
  print(receipt,flush=True)
  (git/file).write_text(original[file])
  assert receipt['detected'],text[-3000:]
finally:
 for file,content in original.items():(git/file).write_text(content)
