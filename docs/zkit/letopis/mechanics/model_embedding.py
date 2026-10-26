"""Embedding work derived from data (D-92/D-95): texts without a vector, live
re-read with search_hash check, attempts in vectors.db; no provider calls."""
from itertools import permutations, product


def drain(nodes, disk, vectors, config, broken=False, fault=None, budget=20):
    """One drain run. nodes: id -> published search_hash (cache); disk: id ->
    live text hash. Returns stored vectors {(config, hash): text_hash} and
    whether the run terminated."""
    bad = lambda name: broken or fault == name
    skipped, steps = set(), 0
    while True:
        steps += 1
        if steps > budget: return vectors, False
        work = sorted(n for n, h in nodes.items()
                      if (config, h) not in vectors and (bad('skip_once') or n not in skipped))
        if not work: return vectors, True
        node = work[0]; wanted = nodes[node]; live = disk[node]
        if live != wanted and not bad('stale_text'):
            skipped.add(node); continue          # the next call publishes the new text
        vectors = dict(vectors); vectors[(config, wanted)] = live


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []

    # 1. The agent edited n1 after the last publication: its live text no longer
    # matches the published search_hash. Drain must not store the new text under
    # the old key, and must still terminate.
    vectors, done = drain({'n1': 'h1', 'n2': 'h2'}, {'n1': 'h1b', 'n2': 'h2'}, {}, 'A', broken, fault)
    if any(key[1] != text for key, text in vectors.items()):
        errors.append('vector stored under a search_hash of a different text')
    if not done:
        errors.append('drain looped on a stale text instead of skipping it once')
    if ('A', 'h2') not in vectors:
        errors.append('fresh text not embedded')

    # 2. Config changes while a vector is computed: the result is stored only if
    # vectors.db meta still holds the captured config identity.
    for order in permutations(['read', 'reset', 'publish']):
        if order.index('read') > order.index('publish'): continue
        meta, store, captured = 'A', set(), None
        for event in order:
            if event == 'read': captured = meta
            if event == 'reset': meta, store = 'B', set()
            if event == 'publish' and (bad('config_meta') or meta == captured):
                store.add((captured, 't'))
        if any(c != meta for c, _ in store):
            errors.append('vector of a previous config stored')

    # 3. Two processes observed a stale identity; the second reset re-checks
    # identity inside its vectors.db write transaction and keeps fresh vectors.
    meta, store = 'B', set()             # first reset
    store.add(('B', 't1'))               # drain stores under the new config
    if bad('reset_recheck') or meta != 'B':
        store = set()                    # duplicate reset from a stale observation
    if ('B', 't1') not in store:
        errors.append('duplicate reset wiped vectors of the current config')

    # P observes A (or an absent store), then reads config A and pauses. Q installs
    # B and stores an exact vector and a reserved failure. P must reject its old
    # observation, rather than resetting B back to A. The oracle is independent
    # literal state, not another call to the reset algorithm.
    for observed in ('A', None):
        desired, meta = 'A', 'B'
        store = {('B', 't1'): (4.0, 5.0, 6.0)}
        failures = {'t2': (1, 'safe provider failure')}
        if meta != desired and (bad('observed_identity') or meta == observed):
            meta, store, failures = desired, {}, {}
        if store.get(('B', 't1')) != (4.0, 5.0, 6.0):
            errors.append('obsolete observation wiped the literal B vector')
        if failures.get('t2') != (1, 'safe provider failure'):
            errors.append('obsolete observation wiped the B failure attempt')
        if meta != 'B':
            errors.append('obsolete observation replaced B identity')

    # 4. Cache rebuilt with vectors.db intact: work is only texts without a vector;
    # identical texts share one vector.
    store = {('A', 't1'), ('A', 't2')}
    nodes = {'n1': 't1', 'n2': 't2', 'n3': 't3', 'n4': 't1'}
    work = {n for n, t in nodes.items() if bad('rebuild') or ('A', t) not in store}
    if work != {'n3'}:
        errors.append('unchanged text recomputed after cache rebuild')

    # 5. Attempts are reserved in vectors.db before the HTTP call, so crashes and
    # restarts never exceed 1+max_retries calls per text.
    for max_retries in range(4):
        limit = 1 + max_retries
        for events in product(['before_http', 'after_http', 'error'], repeat=limit + 2):
            attempts, calls = 0, 0
            for event in events:
                if attempts >= limit: break
                if not bad('attempts'): attempts += 1
                if event != 'before_http': calls += 1
            if calls > limit:
                errors.append('restart exceeded attempt budget %d' % limit)
    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True),
                      'observed_identity_negative_control': run(fault='observed_identity')},
                     indent=2))
