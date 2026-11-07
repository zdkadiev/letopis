"""One fresh-process search call; coordinator serializes trials after builds/tests."""
from pathlib import Path
import argparse,json,sys,time,sqlite3,hashlib,os
root=Path.cwd();sys.path.insert(0,str(root/'tests/performance'))
from measure import measure_process
from scenarios import get_environment,encode_json,write_json,hash_file
parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--source',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--label',required=True);parser.add_argument('--phase',choices=('conditioning','screen','measured'),required=True);parser.add_argument('--mode',choices=('hybrid','fts'),default='hybrid');args=parser.parse_args()
base=root/'.artifacts/issue-65/search-optimization-2026-11-06';baseline=json.loads((base/'baseline-build.json').read_text());output=args.output.absolute();assert not output.exists();output.mkdir(parents=True)
repo=(root/'.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository' if args.mode=='hybrid' else root/'.artifacts/issue-65/2026-11-06-100k-fts600/case-inspect-0')
binary=args.binary.absolute();source=args.source.absolute();inputs={name:hash_file(source/name) for name in baseline['inputs']}
config=json.loads((repo/'.letopis/config.json').read_text());assert config['embedding_model']['enabled'] is (args.mode=='hybrid')
c=sqlite3.connect((repo/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True)
known={row[0] for row in c.execute("select uri from nodes where kind in ('vertex','metavertex')")};assert len(known)==100000
counts={'knowledge':len(known),'source':c.execute("select count(*) from nodes where kind='source'").fetchone()[0],'edges':c.execute('select count(*) from edges').fetchone()[0]};c.close();assert counts=={'knowledge':100000,'source':100000,'edges':500000}
def vector_state():
 c=sqlite3.connect((repo/'.letopis/vectors.db').as_uri()+'?mode=ro',uri=True)
 keys,attempts=c.execute('select (select count(*) from vector_keys),(select count(*) from embed_failures)').fetchone();c.close();return {'vectors':keys,'attempt_rows':attempts}
coverage=vector_state()
if args.mode=='hybrid':assert coverage=={'vectors':100000,'attempt_rows':0},coverage
request={'query':'case 000000','limit':5};pin=hash_file(binary)
env=get_environment()
for name in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(name,None)
env['NO_PROXY']='127.0.0.1,localhost';env['no_proxy']='127.0.0.1,localhost'
manifest={'label':args.label,'phase':args.phase,'mode':args.mode,'binary':str(binary),'binary_sha256':pin,'source':str(source),'inputs':inputs,'repository':str(repo),'config':config,'coverage':coverage,'counts':counts,'request':request,'cache_state':('conditioning with uncontrolled model/OS cache' if args.phase=='conditioning' else 'post-conditioning populated corpus and model; uncontrolled OS cache; fresh CLI process'),'scope':'CLI wait4 resources exclude Ollama; shared host; no intentional owned compilation/tests during serial campaign','started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_average_before':os.getloadavg()}
write_json(output/'manifest.json',manifest)
# Startup-only conditioning avoids attributing first executable assessment to search.
startup=measure_process([str(binary),'--help'],repo,b'',{'timeout':120,'environment':env});write_json(output/'startup.json',startup);assert startup['outcome']=='success'
r=measure_process([str(binary),'search'],repo,encode_json(request),{'timeout':300,'environment':env});r.update(label=args.label,phase=args.phase,mode=args.mode,load_average_after=os.getloadavg());write_json(output/'raw-measurement.json',r)
try:
 response=json.loads(bytes.fromhex(r['stdout']['capture_hex']));items=response.get('result',[]);ids={x['element_id'] for x in items};expected='knowledge/part-0000.md#node-000000'
 assert r['outcome']=='success' and response['ok'] and not response.get('warnings'),response
 assert expected in ids and ids.issubset(known) and all(not x['flags'] for x in items),items
 if args.mode=='hybrid':assert len(items)==len(ids)==5 and len(ids-{expected})==4,items
 else:assert len(items)==len(ids)==1,items
 assert hash_file(binary)==pin
 assert vector_state()==coverage
 r['validation']={'valid':True,'oracle':'independently known literal FTS URI retained; hybrid additionally requires four other known current URIs; counts and vector state stable','full_channel_execution':'requires separate Samply/source evidence, not proved by ordinary result membership alone'}
except Exception as error:
 r['validation']={'valid':False,'error_type':type(error).__name__};write_json(output/'measurement.json',r);raise
write_json(output/'measurement.json',r);print(json.dumps({'label':args.label,'phase':args.phase,'mode':args.mode,'seconds':r['duration_seconds'],'valid':True,'cpu':r['resources']['cpu_seconds']['value'],'rss':r['resources']['peak_rss']['value']}),flush=True)
