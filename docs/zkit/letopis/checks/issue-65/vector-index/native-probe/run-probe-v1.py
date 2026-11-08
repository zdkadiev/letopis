from pathlib import Path
import hashlib,json,re,resource,subprocess,time

ROOT=Path(__file__).resolve().parent
SQLITE=Path('/opt/homebrew/opt/sqlite/bin/sqlite3')
EXT=Path('/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib')
QUERY=ROOT/'query.f32'


def run_sql(name,mode,sql,readonly=True):
    script=f'.bail on\n.load {EXT} sqlite3_extension_init\n'+sql
    (ROOT/f'{name}.sql').write_text(script)
    args=[str(SQLITE)]+(['-readonly'] if readonly else [])+[str(ROOT/f'{mode}.db')]
    before=resource.getrusage(resource.RUSAGE_CHILDREN);start=time.monotonic()
    p=subprocess.run(args,input=script.encode(),capture_output=True,timeout=120)
    elapsed=time.monotonic()-start;after=resource.getrusage(resource.RUSAGE_CHILDREN)
    (ROOT/f'{name}.stdout').write_bytes(p.stdout);(ROOT/f'{name}.stderr').write_bytes(p.stderr)
    receipt={'name':name,'mode':mode,'argv':args,'exit_code':p.returncode,'wall_seconds':elapsed,'process_user_seconds':after.ru_utime-before.ru_utime,'process_system_seconds':after.ru_stime-before.ru_stime,'stdout':p.stdout.decode(),'stderr':p.stderr.decode()}
    timer=re.findall(r'Run Time:\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys\s+([0-9.]+)',receipt['stdout'])
    receipt['sqlite_timers']=[{'wall_seconds':float(a),'user_seconds':float(b),'system_seconds':float(c),'cpu_seconds':float(b)+float(c)} for a,b,c in timer]
    (ROOT/f'{name}.receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    if p.returncode: raise RuntimeError(receipt)
    print(name,elapsed,receipt['sqlite_timers'],flush=True)
    return receipt


def query(name,mode,k):
    out=ROOT/f'{name}.tsv'
    receipt=run_sql(name,mode,f'''.mode tabs
.output {out}
.timer on
select rowid,printf('%!.17g',distance) from vector_index(readfile('{QUERY}'),'{{K:{k}}}') order by distance,rowid;
.timer off
.output stdout
''')
    receipt['K']=k;receipt['rows']=len(out.read_text().splitlines());receipt['output_sha256']=hashlib.file_digest(out.open('rb'),'sha256').hexdigest()
    (ROOT/f'{name}.receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    assert receipt['rows']==k,(name,receipt['rows'])
    assert len(receipt['sqlite_timers'])==1,name
    return receipt

if __name__=='__main__':
    assert len(QUERY.read_bytes())==3072
    run_sql('rebuild-flat','flat',".timer on\ninsert into vector_index(cmd,arg) values('rebuild','{index:\"flat\",distance:\"cos\"}');\n.timer off\n",readonly=False)
    for mode in ['none','flat']:
        run_sql(f'metadata-{mode}',mode,".mode json\nselect count(*) as keys from vector_keys;\nselect hex(val) as model_header from vector_index_model;\nselect count(*) as idx_blocks,sum(length(val)) as idx_bytes from vector_index_idx;\nselect count(*) as base_rows,sum(length(vector)) as base_bytes from vector_index_base;\nselect id,val from vector_index_config;\npragma quick_check;\n")
        query(f'warmup-{mode}',mode,5)
    records=[]
    for k in [5,100,1000,100000]:
        for pair in range(3):
            modes=['none','flat'] if pair%2==0 else ['flat','none']
            for mode in modes: records.append(query(f'k{k}-pair{pair}-{mode}',mode,k))
    (ROOT/'runs.json').write_text(json.dumps(records,indent=2)+'\n')
    print('NATIVE CPU EXPERIMENT STOPPED',flush=True)
