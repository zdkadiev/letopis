from pathlib import Path
import json,re,tempfile,subprocess,os,hashlib
root=Path('/Users/zdkadiev/Repos/letopis');os.chdir(root);base=root/'.artifacts/issue-65/bounded-search-2026-11-07';out=base/'knowledge'
p=Path('letopis/architecture/search-pipeline.md');old=p.read_text();metadata=old.splitlines()[0]
p.write_text(metadata+'''
# Search pipeline

A search request requires a query. An omitted limit defaults to100; an explicit
positive limit uses min(limit,100). Zero/null/invalid numeric forms are errors.
The same effective L bounds each SQL channel and the final response.

Search refreshes the projection, validates all requested source revisions and
captures configuration before its pinned read. Query tokens form a bound literal
FTS AND expression; unavailable query embeddings retain the FTS fallback.
FTS5/BM25 and vec1 cosine matching, joins, ordering and top-L selection execute
inside SQLite. Rust decodes only selected knowledge IDs and ranking metadata:
at most L per channel, at most2L unique candidates. It loads no complete vector
or FTS result collection. SQLite's own exact scan/workspace may still cover all
vectors; final SQL limiting follows current-node joins and URI tie ordering.

Only selected candidates receive fresh knowledge/provenance observations. Filters
apply inside that pool without refill; lower candidates cannot fill an excluded
head. Eligible ranks are recomputed per channel, fused by sum(1/rank), then limited.
Ties use knowledge ID. This replaces full-corpus eligible RRF and tail-candidate
error observation; global refresh/revision/SQL failures still apply. No new ANN,
runtime, embedding identity reset or physical vector ownership change is involved.

The synthetic100k corpus with real768-dimensional Gemma vectors, query case000000,
measured three ordinary pairs per L: L5 wall median66.622→1.344s (49.59x), L100
70.915→1.867s (37.99x). All six outputs per L match byte-for-byte in this scenario;
that is not a general full-corpus ranking equivalence or relevance-quality proof.
Separate genuine Samply at L100 shows refresh44.846%, vector retrieval37.263%,
search eligibility7.626% of observed CPU; inclusive scopes overlap, not wall shares.
Actual-root make verify and host storage acceptance passed. Measurements are warm
local observations, not a Linux/minimum-hardware gate; no document drain was timed.

[Search service](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")
[CLI defaults and validation](../../src/cli/input.rs "letopis:type=external;path=src/cli/input.rs")
[Shared request limit](../../src/service/types.rs "letopis:type=external;path=src/service/types.rs")
[FTS SQL](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")
[Vector SQL](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")
[Agreed design](../../docs/superpowers/specs/2026-11-07-bounded-search-design.md "letopis:type=external;path=docs/superpowers/specs/2026-11-07-bounded-search-design.md")
[Paired measurements](../../docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json")
[Genuine sampled CPU](../../docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json")
[Host verification](../../docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json")

[Full report](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md)
[Earlier research](../../docs/zkit/letopis/checks/issue-65/search-next/report.md)
[Embedding execution](embedding-execution.md "letopis:type=related_to;path=letopis/architecture/embedding-execution.md")
''')
p=Path('letopis/architecture/storage.md');s=p.read_text();old=s
s=s.replace('actual-root host verification passed; measured whole-CLI\nresults below establish no wall-time improvement on the tested corpus.','actual-root host verification passed; the index-only whole-CLI\nexperiment below established no wall-time improvement on the tested corpus.')
# Preserve historical native/index-only measurements while distinguishing the new query contract.
s+='''
## Bounded SQL retrieval after the index experiment

D-108 now uses L=min(requested limit or100,100) for both SQL channels and final
output. FTS and vector SQL return at most L knowledge hits after ordering/joins;
Rust checks only their bounded union, without filter refill. Native K stays full
inside SQLite for current-node coverage and URI ties; this is not an ANN or vector
storage migration. On the same real-vector100k corpus, three pairs measured
66.622→1.344s at L5 and70.915→1.867s at L100; this supersedes the old index-only
whole-search result as the latest query measurement. The old native speedup and
population/write-layout limitations remain historical facts, not new drain data.
Original and owned100k payloads retain all307200000 bytes and their ordered hash.

[Bounded SQL measurements](../../docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json")
[Bounded search contract](../../docs/superpowers/specs/2026-11-07-bounded-search-design.md "letopis:type=external;path=docs/superpowers/specs/2026-11-07-bounded-search-design.md")
[Original vector preservation](../../docs/zkit/letopis/checks/issue-65/bounded-search/vector-preservation/summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/vector-preservation/summary.json")
'''
p.write_text(s)
owners=[Path('letopis/architecture/search-pipeline.md'),Path('letopis/architecture/storage.md')];bindings={}
for owner in owners:
 addresses=re.findall(r'"letopis:type=external;path=([^"\n]+)"',owner.read_text());assert len(addresses)==len(set(addresses)),owner
 bindings[str(owner)]=addresses
paths=sorted({a.split('#L')[0] for values in bindings.values() for a in values});before=[]
for owner in owners:
 meta=json.loads(owner.read_text().splitlines()[0][len('<!-- letopis: '):-len(' -->')]);oldtree=meta['source_snapshot'];changes=[]
 for address in bindings[str(owner)]:
  path=address.split('#L')[0];r=subprocess.run(['git','show',oldtree+':'+path],capture_output=True)
  if r.returncode!=0 or r.stdout!=Path(path).read_bytes():changes.append(address)
 before.append({'owner':str(owner),'old_basis':oldtree,'changed_or_new_sources':changes})
with tempfile.TemporaryDirectory(prefix='letopis-bounded-basis-') as tmp:
 env=dict(os.environ,GIT_INDEX_FILE=str(Path(tmp)/'index'))
 def git(*args):return subprocess.check_output(['git',*args],env=env)
 git('read-tree','HEAD');git('add','--',*paths,'docs/zkit/letopis/checks/issue-65/bounded-search');tree=git('write-tree').decode().strip();assert git('cat-file','-t',tree).strip()==b'tree'
 for path in paths:assert git('show',tree+':'+path)==Path(path).read_bytes(),path
for owner in owners:
 text=owner.read_text();meta=json.loads(text.splitlines()[0][len('<!-- letopis: '):-len(' -->')]);meta['source_snapshot']=tree
 owner.write_text('<!-- letopis: '+json.dumps(meta,separators=(',',':'))+' -->\n'+text.split('\n',1)[1])
(out/'basis.json').write_text(json.dumps({'source_snapshot':tree,'bindings':bindings,'source_sha256':{path:hashlib.sha256(Path(path).read_bytes()).hexdigest() for path in paths},'old_basis_comparison':before,'scope':'Reviewed changed source semantics, compared all current declared source bytes with a complete temporary-index tree; real index and HEAD not staged/committed.'},indent=2)+'\n')
print(tree,{k:len(v) for k,v in bindings.items()})
