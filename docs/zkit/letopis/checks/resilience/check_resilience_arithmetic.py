"""Check illustrative model arithmetic; no runtime/provider/restore measurement."""
import importlib.util
import json
import math
import sys
from pathlib import Path

root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
path = root/'resilience/model.py'
spec = importlib.util.spec_from_file_location('resilience_fixture', path)
model = importlib.util.module_from_spec(spec); spec.loader.exec_module(model)
errors, checks = [], []


def demand(condition, label):
    checks.append(label)
    if not condition: errors.append(label)


a = {k: v['expected'] for k,v in model.ASSUMPTIONS.items()}
r = model.compute(a)
demand(r['illustrative_restore_scope_h'] == 1, 'scope excludes operator/hardware time')
demand(r['illustrative_incident_to_service_h'] == 26, 'end-to-end includes all three phases once')
demand(r['illustrative_loss_window_h'] == 24, 'loss age is independent input, not restore duration')
demand(math.isclose(r['illustrative_downtime_h_month'], 26/12), 'annual incidents converted to monthly downtime')
demand(math.isclose(r['illustrative_availability_pct'], 100*(1-26/8760)), 'availability uses consistent 365-day horizon')
z = dict(a, **{'RA-1':0,'RA-2':0,'RA-3':0,'RA-4':0,'RA-5':0})
rz = model.compute(z)
demand(rz['illustrative_availability_pct'] == 100 and rz['illustrative_downtime_h_month'] == 0, 'zero-outage fixture')
missing = model.compute(dict(a, **{'RA-6':0}))
demand(all(math.isinf(missing[k]) for k in ['illustrative_restore_scope_h','illustrative_incident_to_service_h','illustrative_loss_window_h']), 'no copy yields no finite full-state recovery promise')
demand(missing['illustrative_availability_pct'] == 0, 'unrecoverable event consumes illustrative horizon')
noevent = model.compute(dict(a, **{'RA-6':0,'RA-1':0}))
demand(noevent['illustrative_availability_pct'] == 100 and math.isinf(noevent['illustrative_restore_scope_h']), 'zero times infinity is handled without fabricating restore capability')
huge = model.compute(dict(a, **{'RA-1':1000,'RA-4':1000}))
demand(0 <= huge['illustrative_availability_pct'] <= 100, 'downtime cannot exceed accounting horizon')
older = model.compute(dict(a, **{'RA-5':240}))
demand(older['illustrative_loss_window_h'] == 240 and older['illustrative_availability_pct'] == r['illustrative_availability_pct'], 'older backup changes loss, not uptime')
prep = model.compute(dict(a, **{'RA-3':240}))
demand(prep['illustrative_restore_scope_h'] == r['illustrative_restore_scope_h'] and prep['illustrative_incident_to_service_h'] > r['illustrative_incident_to_service_h'], 'hardware delay separate from scoped restore')
for field, value in [('RA-1',-1),('RA-2',math.nan),('RA-4',math.inf),('RA-6',.5)]:
    try: model.compute(dict(a, **{field:value}))
    except ValueError: demand(True,'reject invalid '+field+' fixture')
    else: demand(False,'reject invalid '+field+' fixture')
demand(model.LIMITS == [], 'no invented SLA or permanent-loss RPO/RTO target')

scenarios = {}
for scenario in ['best','expected','worst']:
    vals = model.compute({k:v[scenario] for k,v in model.ASSUMPTIONS.items()})
    scenarios[scenario] = {k:(v if math.isfinite(v) else 'unbounded') for k,v in vals.items()}
print(json.dumps({'scope':'Illustrative arithmetic only, not operational assessment',
                  'checks':len(checks),'errors':errors,'scenarios':scenarios,
                  'measurements':0,'provider_calls':0},ensure_ascii=False,indent=2,allow_nan=False))
sys.exit(bool(errors))
