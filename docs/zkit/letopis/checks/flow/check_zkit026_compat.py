#!/usr/bin/env python3
"""Control run for the two zkit 0.2.6 checkers that open research directories as files.
Usage: python3 -B check_zkit026_compat.py <project> requirements|architecture
The installed plugin is unchanged. Only the missing directory guard is added in memory.
"""
from pathlib import Path
import sys

project = Path(sys.argv[1]).resolve()
step = sys.argv[2]
if step not in {'requirements', 'architecture'}:
    raise SystemExit('Only requirements/architecture have the diagnosed 0.2.6 issue')
script = Path.home()/'.codex/plugins/cache/personal/zkit/0.2.6/skills'/step/'scripts'/('check_'+step+'.py')
source = script.read_text(encoding='utf-8')
needle = 'for f in files:\n'
if source.count(needle) != 1:
    raise SystemExit('Unexpected upstream source: review compatibility guard before use')
patched = source.replace(needle, needle+'    if not os.path.isfile(os.path.join(rdir, f)): continue\n', 1)
sys.argv = [str(script), str(project)]
exec(compile(patched, str(script), 'exec'), {'__name__':'__main__', '__file__':str(script)})
