"""Illustrative arithmetic only: no measurements, SLA, backup schedule or forecast.

The checker labels the middle fixture 'expected'; here it means a demonstration
input set, NOT an expected operating failure rate or restoration time.
Physical correctness is tested by mechanics models and future DR-1/DR-2.
"""
import math

ASSUMPTIONS = {
    'RA-1': {'label': 'synthetic storage-loss incidents/year', 'unit': '1/year',
             'best': 0, 'expected': 1, 'worst': 4},
    'RA-2': {'label': 'synthetic detection/operator delay', 'unit': 'h',
             'best': 0, 'expected': 1, 'worst': 24},
    'RA-3': {'label': 'synthetic hardware/usable-copy preparation delay', 'unit': 'h',
             'best': 0, 'expected': 24, 'worst': 168},
    'RA-4': {'label': 'synthetic restore-to-graph/FTS after prerequisites ready', 'unit': 'h',
             'best': .1, 'expected': 1, 'worst': 24},
    'RA-5': {'label': 'synthetic age of coherent backup recovery point', 'unit': 'h',
             'best': 0, 'expected': 24, 'worst': 168},
    'RA-6': {'label': 'usable independent copy exists', 'unit': 'bool 0/1',
             'best': 1, 'expected': 1, 'worst': 0},
}
UNITS = {
    'illustrative_restore_scope_h': 'h',
    'illustrative_incident_to_service_h': 'h',
    'illustrative_loss_window_h': 'h',
    'illustrative_downtime_h_month': 'h/month',
    'illustrative_availability_pct': '%',
}


def compute(a):
    for name in ASSUMPTIONS:
        value = a[name]
        if not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
            raise ValueError(name + ' must be finite and nonnegative')
    if a['RA-6'] not in (0, 1):
        raise ValueError('RA-6 must be 0 or 1')
    usable = a['RA-6'] == 1
    restore = a['RA-4'] if usable else math.inf
    incident_to_service = a['RA-2'] + a['RA-3'] + restore
    loss_window = a['RA-5'] if usable else math.inf
    # A finite illustrative one-year accounting horizon. No events means no
    # downtime in that horizon, not an assertion that loss could be restored.
    downtime_year = (0 if a['RA-1'] == 0 else
                     min(8760, a['RA-1'] * incident_to_service))
    return {
        'illustrative_restore_scope_h': restore,
        'illustrative_incident_to_service_h': incident_to_service,
        'illustrative_loss_window_h': loss_window,
        'illustrative_downtime_h_month': downtime_year / 12,
        'illustrative_availability_pct': 100 * (1 - downtime_year / 8760),
    }


# D-81/D-82 deliberately do not promise permanent-loss RPO or restore deadlines.
# No availability SLA exists. Empty limits are NOT a 'fits' verdict. NFR-1 and
# NFR-7 are correctness invariants, not a constant numeric time estimate.
LIMITS = []
