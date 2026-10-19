#!/usr/bin/env python3
"""Validate design schemas/examples only. No letopis runtime, network, models or measurements."""
from pathlib import Path
import json, sys, math, re
import yaml
from jsonschema import Draft202012Validator, FormatChecker
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parents[2]
v=root/'checks/contracts'; schemas=root/'contracts/schemas'; findings=[]; counts={'cli_examples':0,'provider_examples':0,'metadata_examples':0,'assertions':0}
def demand(test,label):
 counts['assertions']+=1
 if not test: findings.append(label)
cli=yaml.safe_load((schemas/'X-1-cli.yaml').read_text());Draft202012Validator.check_schema(cli)
checker=FormatChecker()
def check_cli(case):
 command=cli['x-commands'][case['command']]; target=command[case['kind']]['$ref'].split('/')[-1]
 test=dict(cli,**{'$ref':'#/$defs/'+target});test.pop('anyOf',None)
 errors=list(Draft202012Validator(test,format_checker=checker).iter_errors(case['value']))
 demand((not errors)==case['valid'],case['command']+' '+case['kind']+': '+('; '.join(e.message for e in errors)[:250] if errors else 'invalid example unexpectedly accepted'))
 counts['cli_examples']+=1
for case in json.loads((v/'contracts-examples.json').read_text())['cases']:check_cli(case)
for validpath in ['knowledge/design.md','docs/0x.md','docs/слово.md','docs/sub/../a.md']:
 check_cli({'command':'reindex','kind':'input','valid':True,'value':{'paths':[validpath]}})
for invalidpath in ['/outside.md','docs/\u0000bad.md']:
 check_cli({'command':'reindex','kind':'input','valid':False,'value':{'paths':[invalidpath]}})
provider=yaml.safe_load((schemas/'X-2-embeddings.yaml').read_text())
for endpoint,ops in provider['paths'].items():
 op=ops['post'];demand(bool(op['operationId']),endpoint+' operationId');demand('default' in op['responses'],endpoint+' error response')
 contents=[op['requestBody']['content']['application/json']]+[r['content']['application/json'] for r in op['responses'].values()]
 for content in contents:
  Draft202012Validator.check_schema(content['schema']);errors=list(Draft202012Validator(content['schema']).iter_errors(content['example']))
  demand(not errors,endpoint+' invalid provider example: '+str(errors[:1]));counts['provider_examples']+=1
native=provider['paths']['/embed']['post']['requestBody']['content']['application/json']['schema']
demand(not Draft202012Validator(native).is_valid({'model':'fixture','input':'x','truncate':True}),'native must forbid truncate:true')
# Structural validation cannot enforce correspondence with a separately sent request; this contract check documents it.
def vectors_valid(reply,n,dim,compatible):
 try:
  rows=reply['data'] if compatible else reply['embeddings']
  if compatible:
   indices=[r['index'] for r in rows]
   if len(indices)!=n or set(indices)!=set(range(n)):return False
   vecs=[r['embedding'] for r in rows]
  else:vecs=rows
  return len(vecs)==n and all(len(a)==dim and all(isinstance(x,(int,float)) and not isinstance(x,bool) and math.isfinite(x) for x in a) for a in vecs)
 except (KeyError,TypeError):return False
for value,want in [({'data':[{'index':1,'embedding':[.1,.2]},{'index':0,'embedding':[.3,.4]}]},True),({'data':[{'index':0,'embedding':[.1,.2]},{'index':0,'embedding':[.3,.4]}]},False),({'data':[{'index':0,'embedding':[.1]}]},False)]:
 demand(vectors_valid(value,2,2,True)==want,'provider input/vector correspondence')
md=yaml.safe_load((schemas/'X-4-metadata.yaml').read_text());Draft202012Validator.check_schema(md)
U='08ff5c9e-d4c6-4f73-b0f4-58fd455c6c5c'
metadata_cases=[('canonical',{'id':U,'kind':'vertex','created_at':1792285200},True),('canonical',{'id':'tmp:000','kind':'vertex','created_at':1792285200},False),('input',{'id':'tmp:fff','kind':'metavertex'},True),('input',{'id':'tmp:1000','kind':'vertex'},False),('input',{'id':'tmp:fff\n','kind':'vertex'},False),('canonical',{'id':U.upper(),'kind':'vertex','created_at':0},False),('input',{'id':'tmp:000','kind':'metaedge'},False),('canonical',{'id':U,'kind':'vertex','created_at':0,'types':['decision']},False)]
for name,value,want in metadata_cases:
 test=dict(md,**{'$ref':'#/$defs/'+name});demand(Draft202012Validator(test).is_valid(value)==want,'metadata '+name+' '+repr(value));counts['metadata_examples']+=1
pattern=md['$defs']['edgeTitle']['pattern'];demand(bool(re.fullmatch(pattern,'letopis:id='+U+';type=includes;created_at=1792285200')),'valid canonical edge title');demand(not re.fullmatch(pattern,'letopis:id=a;type=includes;created_at=0'),'reject short canonical edge ID');demand(not re.search(pattern,'letopis:id='+U+';type=includes;created_at=0\n'),'reject trailing newline in canonical edge title')
for field in ['ok','result','error']:demand(field not in cli['$defs']['Configuration']['properties'],'no new envelope inside config input')
text=(root/'contracts.md').read_text();demand('готовыми необходимыми embeddings корпуса' not in text,'stale publication gate');demand('не обещает немедленного появления новой правки в поиске' not in text,'stale apply promise')
result={'scope':'schemas/examples and stated correspondence checks; not letopis implementation or crash validation','counts':counts,'errors':findings,'runtime_calls':0,'measurements':0}
print(json.dumps(result,ensure_ascii=False,indent=2));sys.exit(bool(findings))
