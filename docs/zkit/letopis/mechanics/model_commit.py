"""Finite design model, not filesystem/SQLite implementation or a benchmark."""
from itertools import combinations, permutations, product


def schedules(a, b):
    for slots in combinations(range(len(a) + len(b)), len(a)):
        aa, bb = iter(a), iter(b)
        yield tuple(next(aa) if i in slots else next(bb)
                    for i in range(len(a) + len(b)))


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []
    # Three paths: edited, deleted and newly created. Writes may tear before
    # sync; directories/path absence are represented by dictionary membership.
    before = {'edit.md': b'old\n', 'delete.md': b'keep\n'}
    after = {'edit.md': b'new and longer\n', 'new.md': b'created\n'}
    actions = ['tree', 'journal', 'edit', 'delete', 'create', 'flush', 'commit']
    if bad("journal"):
        actions.insert(actions.index('flush'), 'forget_journal')
    for cut in range(len(actions) + 1):
        files, durable, dirty = dict(before), dict(before), set()
        journal, sealed, db = False, False, 'old'
        for action in actions[:cut]:
            if action == 'tree': sealed = True
            if action == 'journal':
                assert sealed
                journal = True
            if action == 'edit': files['edit.md'] = b'new and longer\n'; dirty.add('edit.md')
            if action == 'delete': files.pop('delete.md', None); dirty.add('delete.md')
            if action == 'create': files['new.md'] = b'created\n'; dirty.add('new.md')
            if action == 'forget_journal': journal = False
            if action == 'flush' and not bad('flush'):
                durable = dict(files); dirty.clear()
            if action == 'commit': db, journal = 'new', False
        # Before flush, a power cut may retain old, new or torn path bytes.
        candidates = [dict(durable)]
        if dirty:
            keys = sorted(dirty)
            values = [list(dict.fromkeys([durable.get(k), files.get(k), b'torn']))
                      for k in keys]
            candidates = []
            for vals in product(*values):
                disk = dict(durable)
                for key, value in zip(keys, vals):
                    if value is None: disk.pop(key, None)
                    else: disk[key] = value
                candidates.append(disk)
        for crashed in candidates:
            expected = after if db == 'new' else before
            if journal:
                # Simulate a second crash after every individual rollback
                # write. The journal remains until all paths are durable.
                restored = dict(crashed)
                for path in ['edit.md', 'delete.md', 'new.md']:
                    if path in before: restored[path] = before[path]
                    else: restored.pop(path, None)
                    replay = dict(restored)
                    for retry_path in ['edit.md', 'delete.md', 'new.md']:
                        if retry_path in before: replay[retry_path] = before[retry_path]
                        else: replay.pop(retry_path, None)
                    if replay != before:
                        errors.append('repeated rollback failed at cut %d' % cut)
                crashed = restored
            if crashed != expected:
                errors.append('crash produced mixed file/DB state at cut %d' % cut)

    # Check -> publish interleavings. Hashes and global includes invariant must
    # be checked while holding the SAME project gate until publication.
    for scenario in ['same_node', 'includes_cycle', 'disjoint']:
        for order in schedules(['A_check', 'A_publish'], ['B_check', 'B_publish']):
            owner, state, accepted, validated = None, {'x': 0, 'y': 0}, [], {}
            edges, deferred = set(), []
            for event in order:
                actor, kind = event.split('_')
                if kind == 'check':
                    if not bad("gate") and owner not in (None, actor):
                        deferred.append(actor)
                        continue
                    owner = actor
                    key = 'x' if scenario == 'same_node' or actor == 'A' else 'y'
                    edge = ('x', 'y') if actor == 'A' else ('y', 'x')
                    valid = state[key] == 0
                    if scenario == 'includes_cycle': valid = (edge[1], edge[0]) not in edges
                    validated[actor] = (valid, key, edge)
                else:
                    if actor not in validated: continue
                    valid, key, edge = validated[actor]
                    if valid:
                        state[key] += 1
                        if scenario == 'includes_cycle': edges.add(edge)
                        accepted.append(actor)
                    owner = None
            # Waiting operations enter after the owner and REVALIDATE current
            # state, rather than reuse a check performed before waiting.
            for actor in deferred:
                key = 'x' if scenario == 'same_node' or actor == 'A' else 'y'
                edge = ('x', 'y') if actor == 'A' else ('y', 'x')
                valid = state[key] == 0
                if scenario == 'includes_cycle': valid = (edge[1], edge[0]) not in edges
                if valid:
                    state[key] += 1
                    if scenario == 'includes_cycle': edges.add(edge)
                    accepted.append(actor)
            if scenario == 'same_node' and len(accepted) > 1:
                errors.append('two stale base hashes both committed: ' + repr(order))
            if ('x', 'y') in edges and ('y', 'x') in edges:
                errors.append('two individually valid includes formed a cycle')
            if scenario == 'disjoint' and len(accepted) != 2:
                errors.append('valid disjoint writes were lost')

    # Independent preconditions: live target, published base, and config.
    # External Git may restore the same target bytes (ABA) after a newer apply.
    for external_target in ['external-X', 'S0']:
        for order in permutations(['prepare', 'external', 'apply', 'config', 'publish']):
            if order.index('prepare') > order.index('publish'): continue
            canonical, published, config = 'S0', 'S0', 'A'
            target, base, basis_config = None, None, None
            for event in order:
                if event == 'prepare':
                    target, base, basis_config = canonical, published, config
                if event == 'external': canonical = external_target
                if event == 'apply': canonical, published = 'S1', 'S1'
                if event == 'config': config = 'B'
                if event == 'publish':
                    valid = (canonical == target and published == base and config == basis_config)
                    if bad("reindex") or valid:
                        if canonical != target:
                            errors.append('reindex target differs from live canonical snapshot')
                        if published != base:
                            errors.append('late reindex regressed a committed apply')
                        if config != basis_config:
                            errors.append('reindex restored vectors/jobs from prior config')
                        published = target
    # Snapshot capture must serialize with the file-changing phase. Raw live
    # reads are deliberately allowed to observe a mixture (D-77).
    live_mixtures = 0
    for order in schedules(['enter', 'patch_a', 'patch_b', 'commit', 'release'],
                           ['capture', 'read_a', 'read_b']):
        files, gate, frozen, pending = ('old', 'old'), False, None, False
        live = {}
        for event in order:
            if event == 'enter': gate = True
            if event == 'patch_a': files = ('new', files[1])
            if event == 'patch_b': files = (files[0], 'new')
            if event == 'capture':
                if gate and not bad("snapshot"): pending = True
                else: frozen = files
            if event == 'release':
                gate = False
                if pending: frozen = files
            if event == 'read_a': live['a'] = files[0]
            if event == 'read_b': live['b'] = files[1]
        if frozen not in [('old', 'old'), ('new', 'new')]:
            errors.append('snapshot captured a mixed in-progress file group')
        if live['a'] != live['b']: live_mixtures += 1
    if not live_mixtures: errors.append('D-77 live-read counterexample not covered')

    # An accepted source basis may differ from before/after trees even when
    # the addressed bytes agree. It must get its OWN reachable durable ref.
    roots = {'before', 'after'}
    source_basis = 'separate-basis-tree'
    if not bad("retention"): roots.add(source_basis)
    retained_after_gc = set(roots)
    if source_basis not in retained_after_gc:
        errors.append('accepted source_snapshot became unreachable after Git GC')
    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True)}, indent=2))
