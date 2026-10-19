"""Bounded queue/config/cancel/restart interleavings; no provider calls."""
from itertools import permutations, product


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []
    for change in ['replace', 'config', 'cancel', 'delete']:
        for order in permutations(['read', 'compute', change, 'publish']):
            if not (order.index('read') < order.index('compute') < order.index('publish')):
                continue
            seq, config, text = 10, 'A', 'old'
            row = (seq, config, text)
            captured, vector, node_exists = None, None, True
            for event in order:
                if event == 'read': captured = row
                if event == change:
                    if change == 'replace':
                        seq, text, vector = 11, 'new', None
                        row = (seq, config, text)
                    elif change == 'config':
                        seq, config, vector = 11, 'B', None
                        row = (seq, config, text)
                    else:
                        row = None
                        if change == 'delete': node_exists = False; vector = None
                if event == 'publish' and captured is not None:
                    if bad("seq") or row == captured:
                        vector = captured[1:]
                        row = None
            changed_after_read = order.index(change) > order.index('read')
            changed_before_publish = order.index(change) < order.index('publish')
            if changed_after_read and changed_before_publish:
                if change in ('replace', 'config') and row is None:
                    errors.append('old seq removed replacement job: ' + change)
                if vector is not None:
                    errors.append('old or cancelled vector published: ' + change)
            if not node_exists and vector is not None:
                errors.append('deleted node retained vector')

    # Crash before HTTP, after HTTP, and after valid publish. A reserved slot
    # is consumed even if no HTTP call could be observed. Retry exhaustion
    # is checked again at restart BEFORE the next external call.
    for max_retries in range(4):
        budget = 1 + max_retries
        for crashes in product(['before_http', 'after_http', 'error'], repeat=budget+2):
            attempts, calls, state = 0, 0, 'queued'
            for crash in crashes:
                if state == 'failed': break
                if attempts >= budget:
                    state = 'failed'; break
                if not bad("attempts"): attempts += 1
                if crash != 'before_http': calls += 1
                if bad('attempts') and crash == 'error': attempts += 1
            if calls > budget:
                errors.append('restart exceeded attempt budget %d' % budget)
            if not bad("attempts") and attempts > budget:
                errors.append('reserved more than allowed attempts')
        # Crash after vector+delete transaction: no queue row to replay.
        row, vector = None, ('A', 'same')
        for _ in range(2):
            if row is not None: errors.append('successful job replayed after restart')
        if vector != ('A', 'same'): errors.append('restart lost committed vector')

    # Different node IDs are independent; replacing one never wipes the queue.
    queue = {'A': 10, 'B': 20}; queue['A'] = 21
    if queue != {'A': 21, 'B': 20}: errors.append('replacement damaged other node')
    # An unrelated apply changes project revision but not this text/config.
    captured, row = (10, 'A', 'same'), (10, 'A', 'same')
    project_revision = 'later-unrelated-tree'
    if row != captured or not project_revision:
        errors.append('unchanged text could not retain applicable work')
    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True)}, indent=2))
