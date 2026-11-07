from pathlib import Path
import re
root=Path(__file__).parent/'source'
names={'text_index_get_by_expression','vector_index_get_by_vector'}
def end_paren(s,start):
 depth=1;quote=False;escape=False
 for i in range(start+1,len(s)):
  c=s[i]
  if quote:
   if escape:escape=False
   elif c=='\\':escape=True
   elif c=='"':quote=False
   continue
  if c=='"':quote=True
  elif c=='(':depth+=1
  elif c==')':
   depth-=1
   if not depth:return i
 raise ValueError('unbalanced')
for p in [*root.glob('src/**/*.rs'),*root.glob('tests/**/*.rs')]:
 s=p.read_text();edits=[]
 for m in re.finditer(r'\b(text_index_get_by_expression|vector_index_get_by_vector)\(',s):
  opening=m.end()-1;end=end_paren(s,opening);args=s[opening+1:end];definition=bool(re.search(r'\bfn\s*$',s[max(0,m.start()-8):m.start()]))
  if definition:
   addition='limit: std::num::NonZeroUsize'
  else:
   addition='std::num::NonZeroUsize::new(100).unwrap()'
   if str(p.relative_to(root)) in ('src/service/search.rs','src/storage/vector.rs') and m.start()<s.find('#[cfg(test)]'):
    addition='limit'
   if str(p.relative_to(root))=='src/service/utils.rs':addition='limit'
   # Trait forwarding functions in test modules preserve the caller's chosen bound.
   line=s[s.rfind('\n',0,m.start())+1:s.find('\n',end)]
   if ('self.inner.' in line or 'StorageRead::' in line) and not str(p.relative_to(root)).startswith('tests/'):
    addition='limit'
  trailing=args.rstrip().endswith(',')
  edits.append((end,('' if trailing else ', ')+addition+(',' if trailing else '')))
 if edits:
  for pos,value in reversed(edits):s=s[:pos]+value+s[pos:]
  p.write_text(s)
print('updated calls and signatures')
