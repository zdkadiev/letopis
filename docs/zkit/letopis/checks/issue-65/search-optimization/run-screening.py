from pathlib import Path
import sys,subprocess,json,time,os,hashlib,urllib.request
root=Path.cwd();p=root/'.artifacts/issue-65/search-optimization-2026-11-06'
b=json.loads((p/'baseline-build.json').read_text());a=json.loads((p/'parse-memo/binary-manifest.json').read_text());m=json.loads((p/'parser-cpu/candidate-build.json').read_text());c=json.loads((p/'combined/build.json').read_text())
# No owned compilers/test runners remain; every candidate is pinned before screening.
variants=[('baseline',Path(b['executable']),root/'.artifacts/issue-65/2026-11-06-native-followup/source'),('parser',Path(m['binary']),p/'parser-cpu/source'),('memo',p/'parse-memo/bin/letopis',p/'parse-memo/source'),('combined',Path(c['binary']),p/'combined/source')]
models=json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open('http://127.0.0.1:11434/api/tags',timeout=5));assert any(x['name']=='embeddinggemma:300m-qat-q4_0' for x in models['models'])
manifest={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'variants':[{'label':label,'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source':str(source)} for label,binary,source in variants],'scope':'serial screening, n=1 per variant after one complete baseline conditioning call; not final comparative speedup evidence','host_shared':True}
(p/'screening-continuation-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def invoke(label,binary,source,phase,output):
 subprocess.run([sys.executable,'-B',str(p/'measure-search.py'),'--binary',str(binary),'--source',str(source),'--output',str(output),'--label',label,'--phase',phase],cwd=root,check=True)
assert (p/'screening/conditioning/validation-recovery.json').is_file()  # Existing successful conditioning is preserved.
for label,binary,source in variants:invoke(label,binary,source,'screen',p/'screening'/label)
print('Serial screening complete',flush=True)
