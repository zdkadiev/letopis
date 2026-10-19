import json,os,signal,subprocess,threading,time,hashlib,platform
from pathlib import Path
ROOT=Path('/private/tmp/engram-sqlite-capacity-idny94a4')
BINARY=ROOT/'target/release/mixed'
DB=ROOT/'capacity.db'
START=time.monotonic();DEADLINE=START+120
summaries=[]
for mode in ['mixed']:
 state={'pid':None,'phase':'starting'};stop=threading.Event();samples=[]
 def monitor():
  while not stop.is_set():
   if state['pid']:
    try:
     raw=subprocess.check_output(['/bin/ps','-p',str(state['pid']),'-o','rss='],text=True,stderr=subprocess.DEVNULL).strip()
     if raw:samples.append({'elapsed_s':time.monotonic()-START,'phase':state['phase'],'pid':state['pid'],'rss_bytes':int(raw)*1024})
    except Exception:pass
   stop.wait(.1)
 with (ROOT/(mode+'-stdout.jsonl')).open('w') as out,(ROOT/(mode+'-time.txt')).open('w') as err:
  proc=subprocess.Popen(['/usr/bin/time','-l','-p',str(BINARY),mode,str(DB),str(ROOT/'vec1.dylib')],stdout=subprocess.PIPE,stderr=err,text=True,start_new_session=True)
  expired=threading.Event()
  def kill():
   expired.set()
   try:os.killpg(proc.pid,signal.SIGKILL)
   except ProcessLookupError:pass
  timer=threading.Timer(max(.1,DEADLINE-time.monotonic()),kill);timer.start()
  thread=threading.Thread(target=monitor,daemon=True);thread.start()
  try:
   for line in proc.stdout:
    out.write(line);out.flush()
    event=json.loads(line)
    if 'pid' in event:state['pid']=event['pid']
    if 'phase' in event:state['phase']=event['phase']
    if event['event'] in ['phase','complete','rebuild','verify','config']:print(json.dumps(event),flush=True)
   code=proc.wait()
  finally:
   timer.cancel();stop.set();thread.join(timeout=2)
 (ROOT/(mode+'-memory.json')).write_text(json.dumps(samples,indent=2))
 result={'mode':mode,'exit_code':code,'deadline_exceeded':expired.is_set(),'observed_max_rss_bytes':max((s['rss_bytes'] for s in samples),default=None),'files':{p.name:p.stat().st_size for p in [DB,Path(str(DB)+'-wal'),Path(str(DB)+'-shm')] if p.exists()}}
 summaries.append(result);(ROOT/'mixed-run-summary.json').write_text(json.dumps({'platform':platform.platform(),'runs':summaries,'elapsed_s':time.monotonic()-START},indent=2))
 if code!=0:raise RuntimeError(result)
print('PROBE_COMPLETE',json.dumps(summaries),flush=True)
