"""Process locks (flock) for index.lock/embed.lock; abstract kernel (D-94/D-95)."""
from collections import deque


def run(broken=False, fault=None):
    bad = lambda name: broken or fault == name
    errors = []
    # D-94: two candidates for the same lock (index.lock or embed.lock). The
    # kernel owns flock and releases it when the holder dies; a leftover lock
    # file or PID must never decide ownership.
    # State: kernel owner, pid-file content, per-candidate phase, crash budget.
    initial = (-1, -1, ('off', 'off'), 0)
    queue, seen = deque([initial]), {initial}
    while queue:
        owner, pidfile, phases, crashes = queue.popleft()
        if sum(x == 'running' for x in phases) > 1:
            errors.append('two holders of one lock admitted')
        for actor in (0, 1):
            phase = phases[actor]
            nxt = []
            if phase == 'off':
                free = (pidfile == -1) if bad('owner') else (owner == -1)
                if free: nxt.append(('running', actor, actor, crashes))
                elif bad('owner') and owner == -1 and pidfile != -1 and all(p == 'off' for p in phases):
                    errors.append('stale lock file blocked every candidate after a crash')
            if phase == 'running':
                nxt.append(('off', -1, -1, crashes))                 # clean exit
                if not crashes:
                    nxt.append(('off', -1, pidfile, 1))              # crash: kernel frees, file stays
            for next_phase, next_owner, next_pid, next_crashes in nxt:
                updated = list(phases); updated[actor] = next_phase
                state = (next_owner, next_pid, tuple(updated), next_crashes)
                if state not in seen: seen.add(state); queue.append(state)

    return sorted(set(errors))


if __name__ == '__main__':
    import json
    print(json.dumps({'violations': run(), 'broken_violations': run(True)}, indent=2))
