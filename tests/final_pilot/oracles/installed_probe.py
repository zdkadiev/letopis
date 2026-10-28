"""Copied behind PREFIX/bin/letopis to observe the installed workflow independently.

The operator prepends PRODUCT and LOG_FILE paths before copying this source. The
participant check script receives the real process output and exit unchanged.
"""
import json
from pathlib import Path
import subprocess
import sys


def main():
    request = sys.stdin.buffer.read()
    cwd = Path.cwd()
    notes = [str(path.relative_to(cwd)) for path in cwd.rglob("*")
             if path.suffix in (".md", ".markdown") and path.is_file()
             and ".git" not in path.parts and ".letopis" not in path.parts]
    result = subprocess.run([PRODUCT, *sys.argv[1:]], input=request, capture_output=True)
    record = {"binary": sys.argv[0], "argv": sys.argv[1:], "cwd": str(cwd),
              "git": (cwd / ".git").is_dir(), "notes": sorted(notes),
              "embedding_config": (cwd / ".letopis/config.json").exists(),
              "stdin": request.decode("utf-8", "replace"), "exit": result.returncode,
              "stdout": result.stdout.decode("utf-8", "replace")}
    with Path(LOG_FILE).open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(record) + "\n")
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
