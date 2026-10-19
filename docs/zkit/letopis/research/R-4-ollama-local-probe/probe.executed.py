import concurrent.futures as cf
import hashlib,json,math,platform,statistics,subprocess,threading,time,urllib.request,urllib.error
from pathlib import Path
OUT=Path('/private/tmp/engram-ollama-probe-20261018')
MODEL='embeddinggemma:300m-qat-q4_0'
BASE='http://127.0.0.1:11434/api/'
lock=threading.Lock(); records=[]; samples=[]; stop=threading.Event()
started=time.monotonic(); deadline=started+175

def api(endpoint,body=None,timeout=10):
 data=None if body is None else json.dumps(body).encode()
 req=urllib.request.Request(BASE+endpoint,data=data,headers={'Content-Type':'application/json'})
 with urllib.request.urlopen(req,timeout=timeout) as r: return json.load(r)

def procs():
 raw=subprocess.check_output(['/bin/ps','-axo','pid=,ppid=,rss=,comm='],text=True)
 rows=[]
 for line in raw.splitlines():
  p=line.strip().split(None,3)
  if len(p)==4 and ('ollama' in p[3].lower()):
   rows.append({'pid':int(p[0]),'ppid':int(p[1]),'rss_bytes':int(p[2])*1024,'command':p[3]})
 return rows

def sample(phase):
 rows=procs()
 return {'elapsed_s':time.monotonic()-started,'phase':phase,'processes':rows,'rss_sum_bytes':sum(p['rss_bytes'] for p in rows)}
phase='baseline'
def monitor():
 while not stop.is_set():
  try:
   entry=sample(phase);samples.append(entry)
   with (OUT/'memory.jsonl').open('a') as f: f.write(json.dumps(entry)+'\n')
  except Exception as e: samples.append({'elapsed_s':time.monotonic()-started,'error':str(e)})
  stop.wait(.5)

def text_for(n,lang,suffix):
 prefix='title: Engram | text: '
 unit=('Knowledge persists in Markdown. Updates use hashes and atomic validation. ' if lang=='en' else 'Знания хранятся в Markdown. Правки проверяют хеши и целостность графа. ')
 text=prefix+(unit*(n//len(unit.encode())+2))
 raw=text.encode()[:n-len(suffix.encode())]
 while True:
  try: return raw.decode()+' '*(n-len(raw)-len(suffix.encode()))+suffix
  except UnicodeDecodeError: raw=raw[:-1]

def embed(text,label,kind,size=None,scheduled=None):
 start=time.monotonic()
 rec={'label':label,'kind':kind,'input_bytes':len(text.encode()),'target_bytes':size,'start_s':start-started}
 if scheduled is not None: rec['dispatch_lag_ms']=(start-scheduled)*1000
 try:
  remaining=deadline-start
  if remaining<=0: raise TimeoutError('probe deadline')
  x=api('embed',{'model':MODEL,'input':text,'truncate':False,'keep_alive':'5m'},timeout=min(35,remaining))
  vectors=x.pop('embeddings',[])
  rec.update(x)
  rec['dimensions']=[len(v) for v in vectors]
  rec['finite']=all(math.isfinite(v) for vs in vectors for v in vs)
  rec['norms']=[math.sqrt(sum(v*v for v in vs)) for vs in vectors]
  rec['ok']=len(vectors)==1 and rec['dimensions']==[768] and rec['finite']
 except urllib.error.HTTPError as e:
  rec.update(ok=False,http_status=e.code,error=e.read().decode())
 except Exception as e: rec.update(ok=False,error=repr(e))
 rec['wall_ms']=(time.monotonic()-start)*1000
 with lock:
  records.append(rec)
  with (OUT/'requests.jsonl').open('a') as f: f.write(json.dumps(rec,ensure_ascii=False)+'\n')
 return rec

metadata={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'platform':platform.platform(),'machine':platform.machine(),'hardware':subprocess.check_output(['/usr/sbin/sysctl','-n','hw.memsize','hw.physicalcpu','machdep.cpu.brand_string'],text=True).splitlines(),'version':api('version'),'before_ps':api('ps'),'model':MODEL}
metadata['tag']=next(x for x in api('tags')['models'] if x['name']==MODEL)
metadata['show']=api('show',{'model':MODEL})
(OUT/'metadata.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2))
if metadata['before_ps'].get('models'):
 raise RuntimeError('Other or preloaded models present: inspect before continuing')
thread=threading.Thread(target=monitor,daemon=True);thread.start()
time.sleep(2)
try:
 phase='first_load'
 print('first_load',json.dumps(embed(text_for(2000,'en','first'),'first_load','document',2000)),flush=True)
 metadata['loaded_ps']=api('ps')
 phase='warm_serial'
 for n in (500,2000,8000):
  for lang in ('en','ru'):
   for i in range(3):
    if time.monotonic()>deadline-65: break
    r=embed(text_for(n,lang,str(i)),f'warm_{n}_{lang}_{i}','document',n)
   print('warm_group',n,lang,flush=True)
 for i in range(10):
  if time.monotonic()>deadline-60: break
  embed('task: search result | query: How do atomic Markdown updates preserve graph consistency? '+str(i),'warm_query','query')
 phase='mixed'
 workload_start=time.monotonic(); planned=15.; submitted=0; skipped=0
 futures=[]
 with cf.ThreadPoolExecutor(max_workers=40) as pool:
  for i in range(150):
   target=workload_start+i/10
   delay=target-time.monotonic()
   if delay>0: time.sleep(delay)
   if time.monotonic()>deadline-35: break
   pending=sum(not f.done() for f in futures)
   if pending>=40:
    skipped+=1;continue
   futures.append(pool.submit(embed,'task: search result | query: Find the previous decision about source hashes. '+str(i),'mixed_query','query',None,target));submitted+=1
   if i%6==0:
    futures.append(pool.submit(embed,text_for(2000,'en',str(i)),'mixed_document','document',2000,target));submitted+=1
  for f in cf.as_completed(futures): f.result()
 metadata['mixed']={'planned_seconds':planned,'actual_elapsed_s':time.monotonic()-workload_start,'query_arrival_per_s':10,'document_arrival_per_min':100,'submitted':submitted,'client_skipped_query':skipped,'client_max_workers':40}
 phase='after';metadata['after_ps']=api('ps');time.sleep(2)
finally:
 stop.set();thread.join(timeout=2)
 (OUT/'metadata.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2))
 (OUT/'memory.json').write_text(json.dumps(samples,ensure_ascii=False,indent=2))
 def summary(rows):
  good=[r['wall_ms'] for r in rows if r.get('ok')]
  good.sort()
  return {'requests':len(rows),'success':len(good),'errors':len(rows)-len(good),'p50_ms':statistics.median(good) if good else None,'p95_ms':good[math.ceil(.95*len(good))-1] if good else None,'max_ms':max(good) if good else None}
 result={'total_elapsed_s':time.monotonic()-started,'groups':{label:summary([r for r in records if r['label']==label]) for label in dict.fromkeys(r['label'] for r in records)},'rss_by_phase':{p:{'samples':len(vals:=[s['rss_sum_bytes'] for s in samples if s.get('phase')==p and 'rss_sum_bytes' in s]),'min_bytes':min(vals) if vals else None,'max_bytes':max(vals) if vals else None} for p in ('baseline','first_load','warm_serial','mixed','after')},'mixed':metadata.get('mixed')}
 (OUT/'summary.json').write_text(json.dumps(result,indent=2))
 print(json.dumps(result,indent=2),flush=True)
