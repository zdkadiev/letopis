import hashlib,json,os,subprocess,time
from pathlib import Path
ROOT=Path(Path('/private/tmp/engram-git-probe-20261018/current-run.txt').read_text())
ENV={k:v for k,v in os.environ.items() if not k.startswith('GIT_')}
ENV.update(GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL=os.devnull,GIT_TERMINAL_PROMPT='0',LC_ALL='C')
START=time.monotonic()
def git(repo,*args):
 p=subprocess.run(['git','-c','gc.auto=0','-c','maintenance.auto=false',*args],cwd=repo,env=ENV,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=10)
 assert p.returncode==0,(args,p.stderr.decode());return p.stdout

def sizes(root):
 files=[p for p in root.rglob('*') if p.is_file()]
 return {'files':len(files),'logical_bytes':sum(p.stat().st_size for p in files),'allocated_file_bytes':sum(p.stat().st_blocks*512 for p in files)}
results=[]
for profile in ('point','rewrite'):
 repo=ROOT/profile;snapshots=json.loads((ROOT/(profile+'-snapshots.json')).read_text());expected=json.loads((ROOT/(profile+'-objects.json')).read_text())
 t=time.monotonic();git(repo,'pack-refs','--all');pack_s=time.monotonic()-t
 refs=dict(line.split() for line in git(repo,'for-each-ref','--format=%(refname) %(objectname)','refs/engram-probe/').decode().splitlines())
 assert refs=={s['ref']:s['tree'] for s in snapshots}
 proc=subprocess.Popen(['git','cat-file','--batch'],cwd=repo,env=ENV,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 try:
  for object_oid,item in expected.items():
   proc.stdin.write((object_oid+'\n').encode());proc.stdin.flush()
   header=proc.stdout.readline().decode().split();assert len(header)==3
   got,kind,length=header;raw=proc.stdout.read(int(length));assert proc.stdout.read(1)==b'\n'
   assert got==object_oid and kind==item['kind'] and len(raw)==item['length']
   assert hashlib.sha256(raw).hexdigest()==item['sha256']
   assert hashlib.sha1(kind.encode()+b' '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==object_oid
  proc.stdin.close();assert proc.wait(timeout=5)==0
 finally:
  if proc.poll() is None: proc.kill();proc.wait()
 git(repo,'fsck','--full','--strict','--no-reflogs')
 packed=repo/'.git/packed-refs'
 result={'profile':profile,'git':sizes(repo/'.git'),'objects':sizes(repo/'.git/objects'),'loose_refs':sizes(repo/'.git/refs'),'packed_refs':{'logical_bytes':packed.stat().st_size,'allocated_file_bytes':packed.stat().st_blocks*512},'refs_checked':len(refs),'objects_checked':len(expected),'all_object_bytes_verified':True,'pack_refs_s':pack_s}
 results.append(result);print(json.dumps(result),flush=True)
(ROOT/'packed-refs-result.json').write_text(json.dumps({'profiles':results,'elapsed_s':time.monotonic()-START},indent=2))
