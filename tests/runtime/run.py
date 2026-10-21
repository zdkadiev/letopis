#!/usr/bin/env python3
"""Build trusted vec1 and run TK-4 checks on temporary corpora and databases."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shlex
import subprocess
import sys
import tempfile
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "embedding"))
from processes import run_command

ROOT = Path(__file__).resolve().parent
TK1 = ROOT.parent / "storage-build"
TK2 = ROOT.parent / "publication"
TK3 = ROOT.parent / "embedding"
REPO = ROOT.parent.parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, help="new directory for logs and manifest")
    args = parser.parse_args()
    if sys.platform not in ("darwin", "linux"):
        raise RuntimeError("TK-4 supports macOS and Linux")
    build = ROOT / ".build"
    build.mkdir(exist_ok=True)
    evidence = (args.evidence or build / datetime.now(timezone.utc).strftime("evidence-%Y%m%dT%H%M%S%fZ")).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    target = ROOT / "target" / f"{sys.platform}-{platform.machine()}"
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_TERM_COLOR="never")
    env.pop("LIBSQLITE3_FLAGS", None)
    env.pop("LETOPIS_VEC1_EXTENSION", None)
    env["TK4_FLOCK_EVIDENCE"] = str(evidence / "flock-observations")
    manifest = {"started_utc": datetime.now(timezone.utc).isoformat(),
                "platform": platform.platform(), "machine": platform.machine(),
                "result": "FAIL", "commands": []}
    manifest["flock_observer"] = ({"method": "native macOS sample call-stack frame", "executable": "/usr/bin/sample", "executable_sha256": digest(Path("/usr/bin/sample"))}
                                  if sys.platform == "darwin" else {"method": "Linux /proc syscall number, LOCK_EX argument and matching index.lock fd device/inode"})

    def execute(argv, *, cwd=ROOT):
        argv = [str(x) for x in argv]
        print(f"$ {shlex.join(argv)}", flush=True)
        number = len(manifest["commands"]) + 1
        log = f"{number:02d}-{Path(argv[0]).name}.txt"
        entry = {"argv": argv, "cwd": str(cwd), "log": log,
                 "environment": {k: env[k] for k in
                                 ("CARGO_TARGET_DIR", "LETOPIS_VEC1_EXTENSION", "CARGO_HOME",
                                  "CARGO_NET_OFFLINE", "RUSTUP_TOOLCHAIN", "TK4_FLOCK_EVIDENCE") if k in env}}
        manifest["commands"].append(entry)
        try:
            result = run_command(argv, cwd=cwd, env=env, timeout=600)
        except subprocess.TimeoutExpired as error:
            entry["timeout_seconds"] = 600
            (evidence / log).write_text((error.stdout or "") + (error.stderr or ""))
            raise RuntimeError(f"command timed out; see {log}") from error
        entry["exit_code"] = result.returncode
        (evidence / log).write_text(result.stderr + result.stdout)
        if argv[:2] == ["cargo", "metadata"]:
            print(f"Cargo metadata saved in {log}", flush=True)
        else:
            print((result.stderr + result.stdout)[-4500:], end="", flush=True)
        if result.returncode:
            raise RuntimeError(f"command exited {result.returncode}; see {log}")
        return result.stdout

    try:
        spec = importlib.util.spec_from_file_location("tk1_runner", TK1 / "run.py")
        tk1 = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(tk1)
        source = tk1.checked_source(build / "vec1.c", tk1.VEC1_URL, tk1.VEC1_SHA256)
        manifest.update(vec1_url=tk1.VEC1_URL, vec1_source_sha256=tk1.VEC1_SHA256)
        rust = execute(["rustc", "--version", "--verbose"])
        if rust.splitlines()[0].split()[1] != "1.95.0":
            raise RuntimeError("Rust 1.95.0 is required")
        execute(["cargo", "--version"])
        compiler = shlex.split(os.environ.get("CC", "cc"))
        execute([*compiler, "--version"])
        metadata = json.loads(execute(["cargo", "metadata", "--locked", "--format-version", "1"]))
        sqlite = next(p for p in metadata["packages"] if p["name"] == "libsqlite3-sys")
        if sqlite["version"] != "0.38.2":
            raise RuntimeError("TK-1 libsqlite3-sys pin changed")
        headers = Path(sqlite["manifest_path"]).parent / "sqlite3"
        manifest.update(libsqlite3_sys=sqlite["version"], sqlite_header_sha256=digest(headers / "sqlite3.h"))
        inputs = [p for p in ROOT.rglob("*") if p.is_file()
                  and not set(p.relative_to(ROOT).parts) & {"target", ".build", "__pycache__"}]
        for dependency in (TK1, TK2, TK3):
            inputs += [p for p in dependency.rglob("*") if p.is_file()
                       and not set(p.relative_to(dependency).parts) & {"target", ".build", "__pycache__"}]
        manifest["files_sha256"] = {os.path.relpath(p, ROOT): digest(p) for p in sorted(inputs)}
        with tempfile.TemporaryDirectory(prefix="native-", dir=build) as temporary:
            library = Path(temporary) / ("vec1.dylib" if sys.platform == "darwin" else "vec1.so")
            execute([*compiler, "-O2", "-DNDEBUG", "-fPIC",
                     "-dynamiclib" if sys.platform == "darwin" else "-shared",
                     "-I", headers, source, "-o", library, "-pthread", "-lm"])
            manifest["vec1_library_sha256"] = digest(library)
            env["LETOPIS_VEC1_EXTENSION"] = str(library)
            execute(["cargo", "run", "--locked", "--bin", "runtime", "--", "--versions"])
            execute(["cargo", "test", "--locked", "--", "--nocapture"])
            execute([sys.executable, "-B", ROOT / "negative_controls.py", "--evidence", evidence / "negative-controls"])
            execute([sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests", "-p", "test_interruptions.py"])
            execute(["cargo", "fmt", "--", "--check"])
            execute(["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"])
        # Complete TK-3 regressions include TK-2 and TK-1, including capability rejection.
        execute([sys.executable, "-B", TK3 / "run.py", "--evidence", evidence / "tk3-regression"], cwd=TK3)
        execute([sys.executable, "-B", REPO / "docs/zkit/letopis/roadmap/schedule.py", "--check"], cwd=REPO)
        manifest["result"] = "PASS"
    except Exception as error:
        manifest["error"] = str(error)
        raise
    finally:
        manifest["finished_utc"] = datetime.now(timezone.utc).isoformat()
        manifest["flock_observations_sha256"] = {str(p.relative_to(evidence)): digest(p) for p in sorted((evidence / "flock-observations").glob("*.txt"))}
        (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"TK-4 {manifest['result']}: {evidence}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        sys.exit(f"TK-4 failed: {error}")
