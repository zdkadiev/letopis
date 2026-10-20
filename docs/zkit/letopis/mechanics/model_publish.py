"""Finite design model of per-file publication (D-93/D-95); not filesystem/SQLite code."""
from itertools import permutations


def publish(files, declared, broken=False, fault=None):
    """One publication unit. files: path -> (valid, links, includes, error) or
    None for a deleted file. Returns declarations and per-file error rows; an
    invalid file keeps its last declaration and stores its error in the same unit."""
    bad = lambda name: broken or fault == name
    new, file_errors = dict(declared), {}
    if bad('per_file') and any(f and not f[0] for f in files.values()):
        return dict(declared), {p: f[3] for p, f in files.items() if f and not f[0]}
    for path in sorted(files):
        f = files[path]
        if f is None: new.pop(path, None); continue
        if not f[0]:
            if not bad('file_error'): file_errors[path] = f[3]
            continue
        new[path] = {'links': set(f[1]), 'includes': set(f[2])}
    return new, file_errors


def traverse(declared, start, broken=False, fault=None, step_limit=50):
    """BFS over stored edges (D-95): edges to missing targets are skipped and
    visited nodes are not expanded again, so an includes cycle terminates."""
    bad = lambda name: broken or fault == name
    seen, queue, out, steps = {start}, [start], [], 0
    while queue:
        node = queue.pop(0); steps += 1
        if steps > step_limit: return out, False
        for kind in ('links', 'includes'):
            for target in sorted(declared.get(node, {}).get(kind, ())):
                if target not in declared and not bad('dangling'): continue
                out.append((node, target, kind))
                if target in seen and not bad('cycle'): continue
                seen.add(target); queue.append(target)
    return out, True


def problems(declared, file_errors):
    """Current problems computed from data (MC-7); nothing stored besides files.error."""
    found = {('file', p) for p in file_errors}
    for path, d in declared.items():
        for kind in ('links', 'includes'):
            found |= {('dangling', path, t) for t in d[kind] if t not in declared}
    return found


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []
    empty = {'links': set(), 'includes': set()}
    base = {'a.md': dict(empty), 'b.md': dict(empty)}

    # 1. Per-file outcome: a valid change is published next to an invalid file,
    # a dangling link and a two-file includes cycle.
    files = {
        'a.md': (True, {'b.md'}, set(), None),
        'b.md': (False, set(), set(), 'bad json'),
        'c.md': (True, {'gone.md'}, set(), None),
        'd.md': (True, set(), {'e.md'}, None),
        'e.md': (True, set(), {'d.md'}, None),
    }
    new, file_errors = publish(files, base, broken, fault)
    if new.get('a.md', {}).get('links') != {'b.md'}:
        errors.append('valid file blocked by an invalid one')
    if new.get('b.md') != base['b.md']:
        errors.append('invalid file replaced its last valid declaration')
    found = problems(new, file_errors)
    if ('file', 'b.md') not in found:
        errors.append('invalid file is not reported while it stays invalid')
    if ('dangling', 'c.md', 'gone.md') not in found:
        errors.append('dangling link not reported')
    edges, _ = traverse(new, 'c.md', broken, fault)
    if ('c.md', 'gone.md', 'links') in edges:
        errors.append('traverse returned an edge to a missing target')
    _, finished = traverse(new, 'd.md', broken, fault)
    if not finished:
        errors.append('includes cycle made traverse run forever')
    # A later publication of the target makes the stored edge usable without
    # touching c.md; deleting it hides the edge again.
    later, later_errors = publish({'gone.md': (True, set(), set(), None)}, new, broken, fault)
    edges, _ = traverse(later, 'c.md', broken, fault)
    if ('c.md', 'gone.md', 'links') not in edges or ('dangling', 'c.md', 'gone.md') in problems(later, later_errors):
        errors.append('edge of an unchanged file not usable after its target appeared')
    fixed, fixed_errors = publish({'b.md': (True, set(), set(), None)}, later, broken, fault)
    if ('file', 'b.md') in problems(fixed, fixed_errors):
        errors.append('fixed file still reported')

    # 2. Two processes see the same delta; an agent edit may land in between.
    # Publication must re-detect under index.lock instead of trusting the delta
    # prepared before waiting, otherwise an older version overwrites a newer one.
    for order in permutations(['p1_detect', 'p2_detect', 'edit', 'p1_publish', 'p2_publish']):
        if order.index('p1_detect') > order.index('p1_publish'): continue
        if order.index('p2_detect') > order.index('p2_publish'): continue
        disk, cache, seen, stale = 'v1', 'v0', {}, False
        for event in order:
            if event == 'edit': disk = 'v2'
            if event.endswith('detect'): seen[event[:2]] = disk
            if event.endswith('publish'):
                cache = seen[event[:2]] if bad('redetect') else disk
                stale = stale or cache != disk   # published state must match disk at that moment
        if stale:
            errors.append('stale prepared delta overwrote a newer publication: ' + ','.join(order))
            break

    # 3. Crash inside the publication unit: the stat prefilter is part of the
    # same unit, so a rolled-back publication is detected again next call.
    for crash_at in ('before_commit', 'after_commit'):
        disk, cache, prefilter = 'v2', 'v1', 'v1'
        if bad('prefilter'): prefilter = 'v2'          # written outside the unit
        if crash_at == 'after_commit': cache, prefilter = 'v2', 'v2'
        if prefilter != disk: cache, prefilter = disk, disk  # next call re-detects
        if cache != disk:
            errors.append('crash left a change that is never republished')

    # 4. Racy mtime: an edit within the same timestamp tick as the last check
    # keeps size+mtime. Files touched in the last tick are rehashed.
    tick = 1
    for edit_offset in (0, 2):
        checked_at, recorded_mtime = 10, 10
        edit_time = checked_at + edit_offset
        disk_mtime = edit_time if edit_offset else recorded_mtime  # same tick
        content_changed = True
        racy = recorded_mtime >= checked_at - tick + 1
        stat_changed = disk_mtime != recorded_mtime
        noticed = stat_changed or (racy and not bad('racy'))
        if content_changed and not noticed:
            errors.append('same-tick edit missed by the stat prefilter')

    # 6. A source_snapshot found in metadata may differ from any captured tree;
    # it must get its own reachable ref before it is relied on.
    roots = {'inspect-snapshot'}
    source_basis = 'separate-basis-tree'
    if not bad("retention"): roots.add(source_basis)
    if source_basis not in roots:
        errors.append('referenced source_snapshot became unreachable after Git GC')
    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True)}, indent=2))
