from pathlib import Path
import subprocess,os,signal,sqlite3,json,time
p=Path.cwd()/'.artifacts/issue-65/2026-11-06-native-followup';evidence=p/'ollama-100k';pid=55679
expected=str(Path(json.loads((p/'before-build.json').read_text())['executable']))+' embed --drain'
command=subprocess.check_output(['ps','-p',str(pid),'-o','command='],text=True).strip()
parent=int(subprocess.check_output(['ps','-p',str(pid),'-o','ppid='],text=True))
assert command==expected and parent==55571,(command,parent)
observations=[];stopped=False
try:
 for attempt in range(100):
  os.kill(pid,signal.SIGSTOP);stopped=True
  for _ in range(100):
   state=subprocess.check_output(['ps','-p',str(pid),'-o','state='],text=True).strip()
   if state.startswith('T'):break
   time.sleep(.001)
  else:raise RuntimeError('owned child did not stop')
  c=sqlite3.connect((evidence/'repository/.letopis/vectors.db').as_uri()+'?mode=ro',uri=True,timeout=.2)
  try:
   keys,reserved,errors=c.execute('select (select count(*) from vector_keys),(select count(*) from embed_failures where error is null),(select count(*) from embed_failures where error is not null)').fetchone()
  finally:c.close()
  observations.append({'attempt':attempt,'vectors':keys,'reserved':reserved,'recorded_errors':errors})
  assert errors==0,'provider error must be investigated rather than reset'
  if reserved==0:
   # No committed attempt can have HTTP in flight. Uncommitted reserve writes roll back.
   os.kill(pid,signal.SIGKILL);stopped=False
   receipt={'kind':'controlled censored segment, not successful drain completion','pid':pid,'expected_command':expected,'parent':parent,'stopped_state':state,'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'monotonic':time.monotonic(),'termination':'SIGKILL only after stopped-state observation and zero committed attempt reservations','database_writes_by_controller':False,'observations':observations}
   (evidence/'controlled-checkpoint.json').write_text(json.dumps(receipt,indent=2)+'\n')
   print(receipt,flush=True);break
  os.kill(pid,signal.SIGCONT);stopped=False;time.sleep(.003)
 else:raise RuntimeError('no safe checkpoint found; original drain remains running')
finally:
 if stopped:os.kill(pid,signal.SIGCONT)
