"""Design artifact checks only; no Letopis runtime, inference or measurements."""
import importlib.util
import json
import sys
from pathlib import Path
from jsonschema import Draft202012Validator
from referencing import Registry, Resource

root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
errors = []
counts = {'internal_cases': 0, 'guard_mutations': 0, 'models': 0}
wire_path = root/'contracts/I-1-worker.yaml'
wire = json.loads(wire_path.read_text())
wire['$id'] = wire_path.as_uri()
Draft202012Validator.check_schema(wire)
external_path = root/'contracts/schemas/X-1-cli.yaml'
registry = Registry().with_resources([
    (wire_path.as_uri(), Resource.from_contents(wire)),
    (external_path.as_uri(), Resource.from_contents(json.loads(external_path.read_text()))),
])


def demand(value, message):
    if not value: errors.append(message)


def check(value, definition, expected=True):
    target = dict(wire)
    target.pop('oneOf', None)
    target['$ref'] = '#/$defs/' + definition
    valid = Draft202012Validator(target, registry=registry).is_valid(value)
    demand(valid == expected, definition + ': ' + repr(value)[:120])
    counts['internal_cases'] += 1


check({'protocol':1,'root':'/Repo/letopis'},'Handshake')
check({'protocol':2,'root':'/Repo/letopis'},'Handshake',False)
check({'protocol':1,'root':''},'Handshake',False)
check({'cancel':True},'Cancel')
check({'cancel':False},'Cancel',False)
check({'cancel':True,'request_id':'extra'},'Cancel',False)
check({'command':'unknown','input':{}},'Request',False)
check({'command':'status','input':{},'extra':True},'Request',False)
for case in json.loads((root/'checks/contracts/contracts-examples.json').read_text())['cases']:
    if case['kind'] == 'input':
        check({'command':case['command'],'input':case['value']},'Request',case['valid'])
    else:
        check(case['value'],'Response',case['valid'])

faults = {
    'model_commit.py':['flush','journal','gate','reindex','snapshot','retention'],
    'model_embedding.py':['seq','attempts'],
    'model_runtime.py':['owner','recovery','journal','clear','waiters','diagnostic_owner'],
}
model_results = {}
for name, mutations in faults.items():
    path = root/'mechanics'/name
    spec = importlib.util.spec_from_file_location(path.stem,path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    good, broken = module.run(), module.run(broken=True)
    demand(not good,name + ' normal: ' + repr(good))
    demand(bool(broken),name + ' broken found no violations')
    mutation_results = {}
    for mutation in mutations:
        violations = module.run(fault=mutation)
        demand(bool(violations),name + ' isolated guard removal undetected: ' + mutation)
        mutation_results[mutation] = violations
        counts['guard_mutations'] += 1
    model_results[name] = {'normal':good,'broken':broken,'isolated_guards':mutation_results}
    counts['models'] += 1

result = {'scope':'Schemas, model invariants and isolated guard removals; not implementation validation',
          'counts':counts,'models':model_results,'errors':errors,'measurements':0,'provider_calls':0}
print(json.dumps(result,ensure_ascii=False,indent=2))
sys.exit(bool(errors))
