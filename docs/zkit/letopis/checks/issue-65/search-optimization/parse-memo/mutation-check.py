from pathlib import Path
import subprocess, os, json
source=Path('src/service/search.rs')
original=source.read_text()
mutants=[
('ignore-content-hash','&& last.file_hash == file_hash','&& (last.file_hash == file_hash || true)','changed_invalid_missing_and_restored_captures_retain_their_flags'),
('ignore-path','&& last.path == *path','&& (last.path == *path || true)','equal_bytes_at_distinct_paths_need_distinct_parses'),
('ignore-binding-predicates','return is_current_knowledge(node, Some(&last.parsed));','return true;','every_binding_still_checks_its_own_hash_and_lines'),
('retain-through-missing','self.last_parsed = None;\n            false','false','missing_capture_discards_the_previous_success'),
('remove-byte-bound','bytes.len() <= MAX_REUSED_FILE_BYTES','(bytes.len() <= MAX_REUSED_FILE_BYTES || true)','oversized_inputs_and_entry_sets_bypass_retention'),
('remove-entry-bound','parsed.elements.len() <= MAX_REUSED_ELEMENTS','(parsed.elements.len() <= MAX_REUSED_ELEMENTS || true)','oversized_inputs_and_entry_sets_bypass_retention'),
('remove-id-bound','id_bytes <= MAX_REUSED_ID_BYTES','(id_bytes <= MAX_REUSED_ID_BYTES || true)','oversized_inputs_and_entry_sets_bypass_retention'),
('retain-search-text','element.search_text = String::new();','let _ = element;','retained_projection_releases_search_and_graph_payloads'),
('retain-graph','parsed.edges.clear();','// deliberately retain graph','retained_projection_releases_search_and_graph_payloads'),
('skip-fresh-capture','let file = map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))','''if self.last_parsed.is_some() {
            return Ok(SearchLink { element_id: node.id.clone(), path, lines: node.lines, revision, flags: BTreeSet::new() });
        }
        let file = map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))''','repeated_bindings_capture_every_time_but_parse_once_per_operation'),
]
results=[]
env=os.environ|{'CARGO_TARGET_DIR':'../target','CARGO_BUILD_JOBS':'2'}
try:
 for name,old,new,test in mutants:
  assert original.count(old)==1,(name,original.count(old))
  source.write_text(original.replace(old,new))
  cmd=['cargo','test','--locked','--offline','--lib',f'service::search::tests::parse_reuse::{test}']
  result=subprocess.run(cmd,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  Path(f'../mutant-{name}.log').write_text(result.stdout)
  caught=result.returncode==101 and 'test result: FAILED.' in result.stdout
  results.append({'name':name,'test':test,'exit':result.returncode,'caught':caught})
  Path('../mutations.json').write_text(json.dumps(results,indent=2)+'\n')
  print(name,'CAUGHT' if caught else 'NOT CAUGHT',flush=True)
  source.write_text(original)
finally:
 source.write_text(original)
assert all(r['caught'] for r in results)
