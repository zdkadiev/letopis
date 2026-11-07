#!/bin/zsh
# Run from a normal macOS Terminal. No global security or signing changes.
set -euo pipefail
repo_root="${0:A:h:h:h:h:h:h:h}"
corpus="$repo_root/.artifacts/issue-65/2026-11-06-100k-fts600/case-inspect-0"
binary="$repo_root/.artifacts/issue-65/2026-11-05-optimized-build/product/letopis"
samply="${SAMPLY_BINARY:-/Users/zdkadiev/.cargo/bin/samply}"
output="${1:-$repo_root/.artifacts/issue-65/samply-terminal-$(date -u +%Y%m%dT%H%M%SZ)}"
export NO_PROXY=127.0.0.1,localhost
python3 - "$repo_root" "$corpus" "$binary" "$output" <<'PY'
import hashlib,json,sys
from pathlib import Path
root,corpus,binary,output=map(Path,sys.argv[1:])
assert root.joinpath('Cargo.toml').is_file(), 'cannot locate Letopis root'
assert not output.exists(), 'use a new output directory'
assert output.is_absolute() and '..' not in output.parts, 'absolute normalized output required'
assert not any(p.is_symlink() for p in (output,*output.parents)), 'symlink output refused'
assert hashlib.sha256(binary.read_bytes()).hexdigest()=='77f8a66d0b0edf76799cb562537839d78fca20a2e78784c7dfe434ec9a8c7e8f', 'benchmark binary drift'
assert json.loads((corpus/'.letopis/config.json').read_text())['embedding_model']['enabled'] is False, 'expected model-free benchmark fixture'
output.mkdir(mode=0o700)
PY
cd "$corpus"
"$samply" --version > "$output/samply-version.txt"
record_command() {
  local command_name="$1"
  print -r -- "Recording $command_name into $output/$command_name.json.gz"
  if "$samply" record --save-only --rate 1000 --unstable-presymbolicate \
    --output "$output/$command_name.json.gz" "$binary" "$command_name" <<<'{}' \
    > "$output/$command_name-result.json" 2> "$output/$command_name.log"; then
    return 0
  else
    local recording_exit=$?
    cat "$output/$command_name.log" >&2
    print -u2 -r -- "Recording failed; receipts remain in $output"
    return "$recording_exit"
  fi
}
record_command check
record_command status
PYTHONPATH="$repo_root/tests/performance" python3 - "$output" "$corpus" <<'PY'
import json,sqlite3,sys
from pathlib import Path
from profile import validate_profile
output,corpus=map(Path,sys.argv[1:])
summary={}
for name in ('check','status'):
 result=json.loads((output/f'{name}-result.json').read_text())
 assert result.get('ok') is True and not result.get('warnings'), result
 if name=='check':assert result['result']['errors']==[]
 else:assert result['result']['problems']==[] and result['result']['locks']==[]
 summary[name]=validate_profile(output/f'{name}.json.gz')
connection=sqlite3.connect((corpus/'.letopis/cache.db').as_uri()+'?mode=ro',uri=True)
kinds=dict(connection.execute('select kind,count(*) from nodes group by kind'))
assert kinds.get('vertex',0)+kinds.get('metavertex',0)==100000 and kinds.get('source',0)==100000
assert connection.execute('select count(*) from edges').fetchone()[0]==500000
connection.close()
(output/'validation.json').write_text(json.dumps(summary,indent=2)+'\n')
print('Validated Samply profiles:',output)
PY
