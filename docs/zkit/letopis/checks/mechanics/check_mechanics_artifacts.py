"""Design artifact checks only; no Letopis runtime, inference or measurements."""
import importlib.util
import json
import sys
from pathlib import Path

root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
errors = []
counts = {'internal_cases': 0, 'guard_mutations': 0, 'models': 0}
def demand(value, message):
    if not value: errors.append(message)


# D-94: no worker wire protocol (I-1-worker.yaml removed); only model checks remain.

faults = {
    'model_publish.py':['per_file','file_error','dangling','cycle','redetect','prefilter','racy','retention'],
    'model_embedding.py':['stale_text','skip_once','config_meta','reset_recheck','rebuild','attempts'],
    'model_runtime.py':['owner'],
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
