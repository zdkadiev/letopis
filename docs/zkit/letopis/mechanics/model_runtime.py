"""Owner/endpoint and diagnostic storage units, abstract kernel and SQLite."""
from itertools import permutations
from collections import deque


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []
    # Exhaust enabled transitions of two candidates with at most one crash.
    # State: kernel owner, socket owner, per-candidate phase, crash budget.
    initial = (-1, -1, ('off', 'off'), 0, (False, False), True, False)
    queue, seen = deque([initial]), {initial}
    while queue:
        owner, endpoint, phases, crashes, recovered, journal, diagnostic = queue.popleft()
        if sum(x != 'off' for x in phases) > 1:
            errors.append('two worker owners admitted')
        for actor in (0, 1):
            phase = phases[actor]
            transitions = []
            if phase == 'off' and (owner == -1 or bad("owner")):
                transitions.append(('locked', actor, endpoint, crashes))
            if phase == 'locked' and (owner == actor or bad("owner")):
                if endpoint != -1 and endpoint != actor and phases[endpoint] != 'off':
                    errors.append('candidate unlinked a live worker endpoint')
                transitions.append(('bound', owner, actor, crashes))
            if phase == 'bound':
                transitions.append(('recovering' if journal else 'ready', owner, endpoint, crashes))
                if bad("recovery"):
                    transitions.append(('serving', owner, endpoint, crashes))
            if phase == 'recovering':
                transitions.append(('ready', owner, endpoint, crashes))
            if phase == 'ready': transitions.append(('serving', owner, endpoint, crashes))
            if phase in ('ready', 'serving'):
                if owner != actor or endpoint != actor:
                    errors.append('ready worker lost kernel/endpoint ownership')
                if not recovered[actor]:
                    errors.append('served command before recovery completed')
                if journal:
                    errors.append('served while unfinished recovery journal exists')
            if not journal and not diagnostic:
                errors.append('bootstrap lost recovery diagnostic')
            if phase != 'off' and not crashes:
                transitions.append(('off', -1 if owner == actor else owner,
                                    endpoint, 1))
            for next_phase, next_owner, next_endpoint, next_crashes in transitions:
                updated = list(phases); updated[actor] = next_phase
                next_recovered = list(recovered)
                if next_phase == 'ready': next_recovered[actor] = True
                if next_phase == 'off': next_recovered[actor] = False
                next_journal, next_diagnostic = journal, diagnostic
                if phase == 'recovering' and next_phase == 'ready':
                    next_journal, next_diagnostic = False, True
                nxt = (next_owner, next_endpoint, tuple(updated), next_crashes,
                       tuple(next_recovered), next_journal, next_diagnostic)
                if nxt not in seen: seen.add(nxt); queue.append(nxt)

    # SQL append/clear/update units may serialize in either order. clear never
    # removes an active error; append after clear must survive that clear.
    for order in permutations(['append_active', 'append_done', 'clear']):
        rows = {}
        for event in order:
            if event == 'append_active': rows[1] = True
            if event == 'append_done': rows[2] = False
            if event == 'clear':
                rows = {} if bad("clear") else {k:v for k,v in rows.items() if v}
        if 1 not in rows: errors.append('clear removed an active error')
        if order.index('append_done') > order.index('clear') and 2 not in rows:
            errors.append('clear removed a later append')
    # Recovery completion unit: journal deletion and diagnostic insert are
    # one atomic DB effect, including crash before/after it.
    actions = ['restore_sync', 'begin', 'append_error', 'delete_journal', 'commit']
    for cut in range(len(actions) + 1):
        journal, diagnostics, restored = True, [], False
        staged_journal, staged_errors = True, []
        for event in actions[:cut]:
            if event == 'restore_sync': restored = True
            if event == 'append_error': staged_errors.append('rollback')
            if event == 'delete_journal':
                if bad("journal"): journal = False  # erroneously outside common tx
                else: staged_journal = False
            if event == 'commit': journal, diagnostics = staged_journal, staged_errors
        # Crash discards staged DB effects. No journal means recovery will NOT
        # run again, so both restored files and the diagnostic must be durable.
        if not journal and not diagnostics: errors.append('rollback diagnostic lost')
        if not journal and not restored: errors.append('journal removed before rollback sync')
    # Cancellation is connection-local and cannot cancel unrelated streams.
    for target in ['A', 'B']:
        active = {'A': True, 'B': True}; active[target] = False
        other = 'B' if target == 'A' else 'A'
        if not active[other]: errors.append('cancellation leaked to other request')
    # Reindex waiters share a task: SIGINT of an attached invocation cancels
    # the linked rebuild, as required by the already agreed X-1 contract.
    for order in permutations(['cancel_A', 'publish']):
        task, reply = 'running', {'A': None, 'B': None}
        for event in order:
            if event == 'cancel_A' and task == 'running':
                task = 'cancelled'
                reply = {'A': 'cancelled', 'B': None if bad("waiters") else 'cancelled'}
            if event == 'publish' and task == 'running':
                task = 'committed'; reply = {'A': 'success', 'B': 'success'}
        if task == 'cancelled' and set(reply.values()) != {'cancelled'}:
            errors.append('reindex waiter did not receive shared cancellation')
    # Diagnostic ownership outlives the queue row. A failed rebuild may remove
    # its row but must remain resolvable after restart. Retry replacement
    # completes the old message, preserving it for explicit clear.
    for order in permutations(['retry_error', 'terminal_remove', 'clear']):
        rows = [(1, 'reindex', True)]
        task_exists = True
        for event in order:
            if event == 'retry_error':
                if not bad('diagnostic_owner'):
                    rows = [(i,o,False if o=='reindex' else a) for i,o,a in rows]
                rows.append((2,'reindex',True))
            if event == 'terminal_remove': task_exists = False
            if event == 'clear': rows = [(i,o,a) for i,o,a in rows if a]
        if not task_exists:
            # Simulate restart, no in-memory task/error ID retained.
            active = [r for r in rows if r[1]=='reindex' and r[2]]
            if len(active) != 1:
                errors.append('retry left orphaned or duplicate active diagnostics')
            rows = [(i,o,False if o=='reindex' else a) for i,o,a in rows]
            if any(a for _,o,a in rows if o=='reindex'):
                errors.append('successful rebuild could not resolve old diagnostic')
    # Stored diagnostics keep their history, but expose the authoritative
    # lifecycle bit in details. Immediate command errors need no such bit.
    for active in (False, True):
        stored = {'code': 'embedding_failed', 'details': {'node_id': 'N'},
                  'active': active}
        public = {'code': stored['code'], 'details': dict(stored['details'])}
        if not bad('diagnostic_state'):
            public['details']['active'] = stored['active']
        if public['details'].get('active') is not active:
            errors.append('status cannot distinguish failed from recovered diagnostics')
        if 'active' in stored['details']:
            errors.append('diagnostic projection duplicated lifecycle state in storage')

    # Inspect can address nodes and edges in one canonical snapshot. Edge
    # location/hash come from the link in its source fragment, not stale SQL.
    canonical = {
        'N': {'path': 'a.md', 'lines': (1, 5), 'hash': 'node-new'},
        'E': {'path': 'a.md', 'lines': (3, 3), 'hash': 'edge-new'},
        'S': {'path': 'missing.rs', 'hash': None},
    }
    for requested in [('N',), ('E',), ('N', 'E', 'S'), ('E', 'E'), ('E', 'unknown')]:
        captured = {k:dict(v) for k,v in canonical.items()
                    if not (bad('edge_inspection') and k == 'E')}
        wanted = set(requested)
        result = None if wanted - captured.keys() else {k:captured[k] for k in wanted}
        if wanted <= canonical.keys():
            if result != {k:canonical[k] for k in wanted}:
                errors.append('snapshot inspection omitted edge metadata or used a different view')
        elif result is not None:
            errors.append('mixed inspect returned a partial batch for an unknown ID')
    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True)}, indent=2))
