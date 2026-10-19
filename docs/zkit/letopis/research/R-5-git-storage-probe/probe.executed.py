"""Reproducible Git storage probe; writes only a fresh temporary directory."""
import hashlib,json,os,platform,random,shutil,subprocess,tempfile,time,uuid
from pathlib import Path
ROOT=Path(tempfile.mkdtemp(prefix='engram-git-probe-run-',dir='/private/tmp'))
START=time.monotonic();DEADLINE=START+120
ENV={k:v for k,v in os.environ.items() if not k.startswith('GIT_')}
ENV.update(GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL=os.devnull,GIT_TERMINAL_PROMPT='0',LC_ALL='C')
WORDS=('agent memory knowledge decision source revision snapshot constraint document graph vertex link index search transaction hash consistency validation atomic update history evidence project function module interface contract requirement implementation test error recovery queue embedding model local markdown repository context relation process result schema operation file semantic lexical reference apply read write persistent storage architecture design assumption reasoning code service event domain limit state metadata version paragraph attribute data stable shared fragment identifier boundary system change analysis hypothesis task session user rule').split()
NFILES=30;NNODES=30;NODE_BYTES=2000;STEPS=200;TOUCHED=5

def checktime():
 if time.monotonic()>=DEADLINE: raise TimeoutError('120-second probe limit')

def git(repo,*args,data=None):
 checktime();t=time.monotonic()
 p=subprocess.run(['git','-c','gc.auto=0','-c','maintenance.auto=false',*args],cwd=repo,env=ENV,input=data,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=max(.1,DEADLINE-time.monotonic()))
 if p.returncode: raise RuntimeError((args,p.returncode,p.stderr.decode()))
 return p.stdout

def oid(kind,data): return hashlib.sha1(kind.encode()+b' '+str(len(data)).encode()+b'\0'+data).hexdigest()

def fragment(file_i,node_i,generation):
 rng=random.Random(720000+file_i*100000+node_i*1000+generation*100000000)
 ident=uuid.UUID(int=random.Random(file_i*NNODES+node_i+125).getrandbits(128),version=4)
 prefix=('<!-- engram: '+json.dumps({'id':str(ident),'kind':'vertex','created_at':1792320000},separators=(',',':'))+' -->\n# Knowledge\n\n').encode()
 suffix=b'\nvalue:0000000000\n<!-- /engram -->\n'
 count=NODE_BYTES-len(prefix)-len(suffix)
 body=(' '.join(rng.choice(WORDS) for _ in range(400))).encode()[:count]
 body+=b' '*(count-len(body))
 result=prefix+body+suffix
 assert len(result)==NODE_BYTES
 return result

def sizes(root):
 files=[p for p in root.rglob('*') if p.is_file()]
 return {'files':len(files),'logical_bytes':sum(p.stat().st_size for p in files),'allocated_file_bytes':sum(p.stat().st_blocks*512 for p in files)}

def storage(repo):
 return {'git':sizes(repo/'.git'),'objects':sizes(repo/'.git/objects'),'refs':sizes(repo/'.git/refs'),'working_markdown_bytes':sum(p.stat().st_size for p in repo.glob('*.md')),'count_objects_v':git(repo,'count-objects','-v').decode()}

def verify(repo,expected,snapshots):
 refs=dict(line.split() for line in git(repo,'for-each-ref','--format=%(refname) %(objectname)','refs/engram-probe/').decode().splitlines())
 assert refs=={s['ref']:s['tree'] for s in snapshots}
 proc=subprocess.Popen(['git','cat-file','--batch'],cwd=repo,env=ENV,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 try:
  for object_oid,item in expected.items():
   checktime();proc.stdin.write((object_oid+'\n').encode());proc.stdin.flush()
   header=proc.stdout.readline().decode().split();assert len(header)==3,(object_oid,header)
   got,kind,length=header;raw=proc.stdout.read(int(length));assert proc.stdout.read(1)==b'\n'
   assert kind==item['kind'] and got==object_oid
   assert hashlib.sha256(raw).hexdigest()==item['sha256']
   assert oid(kind,raw)==object_oid
  proc.stdin.close();assert proc.wait(timeout=5)==0
 finally:
  if proc.poll() is None: proc.kill();proc.wait()
 return {'refs_checked':len(refs),'objects_checked':len(expected),'all_object_bytes_verified':True}

def profile(name):
 t=time.monotonic();repo=ROOT/name;repo.mkdir()
 git(repo,'init','-q','--template=','--object-format=sha1')
 expected={};current={};snapshots=[];file_names=[f'knowledge-{i:03d}.md' for i in range(NFILES)]
 def save_file(i,raw):
  path=repo/file_names[i];path.write_bytes(raw)
  blob=git(repo,'hash-object','-w','--stdin',data=raw).decode().strip()
  assert blob==oid('blob',raw)
  expected[blob]={'kind':'blob','sha256':hashlib.sha256(raw).hexdigest(),'length':len(raw)};current[file_names[i]]=blob
  return f'100644 {blob}\t{file_names[i]}\n'
 def snapshot(step,index_info):
  git(repo,'update-index','--index-info',data=index_info.encode())
  tree=git(repo,'write-tree').decode().strip()
  raw=b''.join(b'100644 '+path.encode()+b'\0'+bytes.fromhex(current[path]) for path in sorted(current))
  assert tree==oid('tree',raw)
  expected[tree]={'kind':'tree','sha256':hashlib.sha256(raw).hexdigest(),'length':len(raw)}
  ref=f'refs/engram-probe/snapshot-{step:04d}';git(repo,'update-ref',ref,tree)
  snapshots.append({'ref':ref,'tree':tree})
 snapshot(0,''.join(save_file(i,b''.join(fragment(i,n,0) for n in range(NNODES))) for i in range(NFILES)))
 initial=storage(repo)
 generation_start=time.monotonic()
 for step in range(STEPS):
  changes=[];node=(step//6)%NNODES
  for j in range(TOUCHED):
   i=(step*TOUCHED+j)%NFILES;path=repo/file_names[i];raw=path.read_bytes();begin=node*NODE_BYTES
   if name=='point':
    part=raw[begin:begin+NODE_BYTES];needle=b'value:';position=part.index(needle)+len(needle)
    part=part[:position]+f'{step+1:010d}'.encode()+part[position+10:]
   else:
    part=fragment(i,node,step+1)
   raw=raw[:begin]+part+raw[begin+NODE_BYTES:];assert len(raw)==NFILES*NODE_BYTES
   changes.append(save_file(i,raw))
  snapshot(step+1,''.join(changes))
 generation_s=time.monotonic()-generation_start
 before=storage(repo);verified_before=verify(repo,expected,snapshots)
 tpack=time.monotonic();git(repo,'repack','-a','-d');pack_s=time.monotonic()-tpack
 after=storage(repo);verified_after=verify(repo,expected,snapshots)
 fsck=git(repo,'fsck','--full','--strict','--no-reflogs').decode()
 result={'profile':name,'repo':str(repo),'steps':STEPS,'files':NFILES,'nodes_per_file':NNODES,'file_bytes':NNODES*NODE_BYTES,'new_file_versions':STEPS*TOUCHED,'new_file_content_bytes':STEPS*TOUCHED*NNODES*NODE_BYTES,'initial':initial,'before_pack':before,'after_pack':after,'verify_before':verified_before,'verify_after':verified_after,'fsck_stdout':fsck,'generation_s':generation_s,'pack_s':pack_s,'total_s':time.monotonic()-t}
 (ROOT/(name+'-objects.json')).write_text(json.dumps(expected,indent=2));(ROOT/(name+'-snapshots.json')).write_text(json.dumps(snapshots,indent=2));(ROOT/(name+'-result.json')).write_text(json.dumps(result,indent=2))
 print(json.dumps(result),flush=True)
 return result

metadata={'date_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'platform':platform.platform(),'git':subprocess.check_output(['git','--version'],env=ENV,text=True).strip(),'fixture':'30 files x 30 nodes x 2000 ASCII bytes; seeded vocabulary text; point changes a 10-digit value; rewrite replaces text with fresh seeded words but stable identity','global_git_config_ignored':True,'commit_count':0,'deadline_s':120,'root':str(ROOT)}
(ROOT/'metadata.json').write_text(json.dumps(metadata,indent=2))
Path('/private/tmp/engram-git-probe-20261018/current-run.txt').write_text(str(ROOT))
try:
 results=[profile('point'),profile('rewrite')]
 (ROOT/'summary.json').write_text(json.dumps({'metadata':metadata,'profiles':results,'elapsed_s':time.monotonic()-START},indent=2))
 print('COMPLETE',str(ROOT),'elapsed_s',round(time.monotonic()-START,3),flush=True)
except Exception as e:
 (ROOT/'failure.json').write_text(json.dumps({'error':repr(e),'elapsed_s':time.monotonic()-START},indent=2));raise
