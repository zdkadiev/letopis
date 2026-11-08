from pathlib import Path
import json,shutil,statistics
root=Path.cwd();base=root/'.artifacts/issue-65/2026-11-06-native-followup';e=base/'ollama-100k';doc=root/'docs/zkit/letopis/checks/issue-65/native-followup'
def get(name):return json.loads((e/name).read_text())
assert get('completion.json')['outcome']=='complete'
c=get('native-complete-coverage.json')[0];first=get('drain.json');resume=get('resume-drain.json');summary=get('full-hybrid-summary.json');native=get('full-native-stacks.json')
assert c['native_vectors']==c['keys']==100000 and c['knowledge_without_vector']==0
interval=resume['started_monotonic']+resume['duration_seconds']-first['started_monotonic'];active=first['duration_seconds']+resume['duration_seconds'];gap=interval-active
cpu=sum(r['resources']['cpu_seconds']['value'] for r in (first,resume))
query_cpu=[get(f'full-hybrid-{n}.json')['resources']['cpu_seconds']['value'] for n in range(3)]
query_rss=max(get(f'full-hybrid-{n}.json')['resources']['peak_rss']['value'] for n in range(3))/2**20
shares={}
for short in ('get_eligible_links','file_parse','get_snapshot_hash','get_current_hash','vector_index_get_by_vector'):
 f=next((x for x in native['functions'] if short in x['function']),None)
 shares[short]=0 if f is None else 100*f['inclusive_cpu_delta']/native['observed_cpu_delta']
text=f'''## Complete 100k Ollama population and real hybrid search

The real local `embeddinggemma:300m-qat-q4_0` run completed with **100000 native
768-dimensional vectors for 100000 knowledge entries**. Independent read-only
SQLite/vec1 verification found no missing vector/key mappings, wrong vector
widths, orphan hashes, knowledge entries without vectors, recorded provider errors
or leftover attempt reservations. The same configuration identity was retained.

Population used two segments, not one uninterrupted latency sample:

| Segment | Outcome | Vectors at end | Measured CLI interval, s |
| --- | --- | ---: | ---: |
| Initial | Controlled censoring, exit -9 | 51024 | {first['duration_seconds']:.3f} |
| Same-config continuation | Successful command completion | 100000 | {resume['duration_seconds']:.3f} |

The first segment was stopped before its 3600-second budget at a verified
zero-reservation checkpoint. SIGSTOP/SIGCONT observations and the termination
receipt are retained. The controller wrote no database data, refunded no attempt
and changed no configuration or retry setting. The initial runner's assertion
failure is expected censoring, not a successful complete drain or provider failure.
The continuation started with all 51024 vectors intact and zero attempt rows.

Total observed population interval including the {gap:.3f}-second restart gap:
**{interval:.3f} s ({interval/60:.2f} min)**, about {100000/interval:.3f} unique
vectors/s. Active CLI intervals sum to {active:.3f} s; their wait4 CPU totals sum
to {cpu:.3f} s. These CLI counters exclude Ollama daemon/model CPU, GPU and memory.
The initial segment overlapped verification, profiling and ordinary comparisons;
the host was shared throughout. This is one split population experiment, with
uncontrolled model/OS caches, not a steady-state or production throughput verdict.

[Native coverage](ollama/native-complete-coverage.json),
[first segment](ollama/drain.json), [checkpoint](ollama/controlled-checkpoint.json),
[continuation](ollama/resume-drain.json), [identity](ollama/complete-coverage.json),
[coverage SQL](coverage.sql), [oracle build](native-oracle-build.json).
The independent coverage scan precedes query timing and reads all vectors; no
query is described as cold.

Three ordinary full-coverage mixed searches used the **new pinned release** and
`case 000000`, limit 5. An independent FTS query finds exactly one literal URI;
each product result retains that URI and adds four other known URIs with clean
flags. Both lexical and vector channels are therefore demonstrated; this is not
FTS fallback. No semantic relevance-quality verdict is inferred.

| Measurement | Result |
| --- | --- |
| Full hybrid CLI samples, s | {', '.join(f'{x:.3f}' for x in summary['samples_seconds'])} |
| Median, s; n=3 | {summary['median_seconds']:.3f} |
| Median CLI CPU, s | {statistics.median(query_cpu):.3f} |
| Maximum observed CLI RSS, MiB | {query_rss:.2f} |
| Enabled-model check, one valid call, s | {get('full-check.json')['duration_seconds']:.3f} |
| Enabled-model status, one valid call, s | {get('full-status.json')['duration_seconds']:.3f} |

The full check/status calls are separate observations, not extra samples in the
paired model-free comparison. Full status reports zero backlog, failed embeddings,
problems and active locks. No owned drain/tests/compilation ran during these full
ordinary reads. n=3 supports no p95/p99 claim. Model resources remain outside CLI
wait4 RSS, and no minimum-hardware, relevance or formal NFR gate is closed.

[Full query summary](ollama/full-hybrid-summary.json),
[full read manifest/oracles](ollama/full-reads-manifest.json),
[full check](ollama/full-check.json), [full status](ollama/full-status.json).
A separate genuine Samply mixed-query recording was validated after ordinary
measurements. Its observed CPU shares are eligibility {shares['get_eligible_links']:.2f}%,
Markdown parsing {shares['file_parse']:.2f}%, historical hashing
{shares['get_snapshot_hash']:.2f}%, live source hashing {shares['get_current_hash']:.2f}%
and native vector retrieval {shares['vector_index_get_by_vector']:.2f}%. These inclusive
scopes overlap and cannot be added. The engine requests K equal to stored vector
count; application eligibility and repeated parsing remain the main search cost.
This was diagnosed and drafted for follow-up, not changed in this implementation.
[Raw profile](ollama/full-native-search.json.gz),
[symbols](ollama/full-native-search.json.syms.json),
[stacks](ollama/full-native-stacks.json), [validation](ollama/full-native-validation.json).

Earlier partial-coverage vector-only probes are retained separately: coverage
20523–23032, first copied-fixture refresh/query 89.680 s, repeats 30.766 and
31.679 s, independent zero-FTS and known-URI oracles. Their pre-memo release,
coverage, concurrency and query differ from the complete-coverage results;
they are not pooled or used as a search before/after speedup.

A separate three-second macOS `sample` window during resumed drain produced
2569 main-thread observations: 1345 in HTTP input waiting (52.36%), 954 in next-work
SQL (37.14%). These are wall-stack observation shares, not a Samply CPU profile
or exact whole-run time fractions. They confirm queue selection as another cost.
[Wall stacks](ollama/resume-wall-sample.txt), [summary](ollama/resume-wall-summary.json).

An embed_failures row with error=null is an attempt reservation, not a provider
error. Early raw progress receipts named that table-row count `failures`; interpret
it as attempt rows. Final reserved and recorded-error counts are both zero.
The API's one-time 302304460-byte resident model allocation observation is not peak
model RSS or simultaneous whole-stack RAM. Paid providers, production pilots,
controlled cold caches and Linux/x86_64 were not measured here.

[Follow-up drafts](follow-ups.md) remain local; no issues or formal gates were closed.
'''
f=doc/'report.md';old=f.read_text();old=old.replace('The real Ollama 100k drain is still running at this report checkpoint. Complete\n100k vector coverage is not yet claimed. The earlier permission failures remain\nhistorical evidence; they do not describe this unrestricted executor.','The real local Ollama population and full-coverage hybrid measurements also\ncompleted. The earlier permission failures remain historical evidence; they do\nnot describe this unrestricted executor.')
head=old.split('## Ollama checkpoint and remaining evidence')[0];assert head!=old
f.write_text(head+text)
for src in e.iterdir():
 if src.is_file():shutil.copy2(src,doc/'ollama'/src.name)
for name in ('full-ollama-reads.py','finish-ollama.py','resume-ollama.py','checkpoint-drain.py','finalize-report.py'):shutil.copy2(base/name,doc/name)
print(json.dumps({'population_interval_seconds':interval,'active_drain_seconds':active,'restart_gap_seconds':gap,'unique_vectors_per_second':100000/interval,'full_hybrid_median_seconds':summary['median_seconds'],'profile_cpu_shares':shares},indent=2))
