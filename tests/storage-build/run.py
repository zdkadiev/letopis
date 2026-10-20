#!/usr/bin/env python3
"""Build and run the TK-1 checks. All generated databases belong to a temporary directory."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent
VEC1_URL = "https://raw.githubusercontent.com/sqlite/sqlite/a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7/ext/vec1/vec1.c"
VEC1_SHA256 = "a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9"


def checked_source(path: Path, url: str, digest: str) -> Path:
    if path.exists():
        data = path.read_bytes()
    else:
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
    actual = hashlib.sha256(data).hexdigest()
    if actual != digest:
        raise RuntimeError(f"SHA-256 mismatch for {path.name}: expected {digest}, found {actual}")
    if not path.exists():
        # Readers see either no cache entry or the complete verified source.
        # Same-directory replacement is atomic on both supported platforms.
        with tempfile.TemporaryDirectory(prefix=".vec1-", dir=path.parent) as temporary:
            staged = Path(temporary) / path.name
            staged.write_bytes(data)
            staged.replace(path)
    return path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, help="new directory for logs and manifest")
    parser.add_argument("--system-sqlite", action="store_true", help="also prove an old system SQLite is rejected")
    args = parser.parse_args()
    if sys.platform not in ("darwin", "linux"):
        raise RuntimeError("TK-1 runner supports macOS and Linux")
    build = ROOT / ".build"
    build.mkdir(exist_ok=True)
    evidence = args.evidence or build / datetime.now(timezone.utc).strftime("evidence-%Y%m%dT%H%M%S%fZ")
    evidence = evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    manifest = {
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "vec1_url": VEC1_URL,
        "vec1_source_sha256": VEC1_SHA256,
        "commands": [],
        "result": "FAIL",
    }
    target = ROOT / "target" / f"{sys.platform}-{platform.machine()}"
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_TERM_COLOR="never")
    # The normal build must not inherit flags used by a previous negative fixture.
    environment.pop("LIBSQLITE3_FLAGS", None)

    def execute(command, *, env=None, rejection=None):
        command = [str(part) for part in command]
        actual_env = env or environment
        print(f"$ {shlex.join(command)}", flush=True)
        result = subprocess.run(command, cwd=ROOT, env=actual_env, text=True, capture_output=True)
        output = result.stderr + result.stdout
        number = len(manifest["commands"]) + 1
        log = f"{number:02d}-{Path(command[0]).name}.txt"
        (evidence / log).write_text(output)
        manifest["commands"].append({
            "argv": command, "exit_code": result.returncode, "log": log,
            "expected_rejection": rejection,
            "environment": {key: actual_env[key] for key in
                            ("CARGO_TARGET_DIR", "LIBSQLITE3_FLAGS", "LETOPIS_VEC1_EXTENSION")
                            if key in actual_env},
        })
        if command[:2] == ["cargo", "metadata"]:
            print(f"Cargo metadata saved in {log}", flush=True)
        else:
            print(output[-3000:], end="", flush=True)
        if rejection:
            if result.returncode == 0 or rejection not in output:
                raise RuntimeError(f"expected nonzero exit and diagnostic {rejection!r}; see {log}")
        elif result.returncode:
            raise RuntimeError(f"command failed with exit {result.returncode}; see {log}")
        return result.stdout

    try:
        source = checked_source(build / "vec1.c", VEC1_URL, VEC1_SHA256)
        execute([sys.executable, "-B", "-m", "unittest", "-v", "test_runner"])
        rust = execute(["rustc", "--version", "--verbose"])
        if rust.splitlines()[0].split()[1] != "1.95.0":
            raise RuntimeError("Rust 1.95.0 is required by the TK-1 toolchain pin")
        execute(["cargo", "--version"])
        compiler = shlex.split(os.environ.get("CC", "cc"))
        execute([*compiler, "--version"])
        metadata = json.loads(execute(["cargo", "metadata", "--locked", "--format-version", "1"]))
        sqlite = next(package for package in metadata["packages"] if package["name"] == "libsqlite3-sys")
        headers = Path(sqlite["manifest_path"]).parent / "sqlite3"
        manifest["libsqlite3_sys"] = sqlite["version"]
        manifest["sqlite_header_sha256"] = hashlib.sha256((headers / "sqlite3.h").read_bytes()).hexdigest()
        manifest["files_sha256"] = {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(ROOT.rglob("*")) if path.is_file()
            and not set(path.relative_to(ROOT).parts) & {"target", ".build", "__pycache__"}
        }
        with tempfile.TemporaryDirectory(prefix="tk1-", dir=build) as temporary:
            scratch = Path(temporary)
            library = scratch / ("vec1.dylib" if sys.platform == "darwin" else "vec1.so")
            execute([*compiler, "-O2", "-DNDEBUG", "-fPIC",
                     "-dynamiclib" if sys.platform == "darwin" else "-shared",
                     "-I", headers, source, "-o", library, "-pthread", "-lm"])
            manifest["vec1_library_sha256"] = hashlib.sha256(library.read_bytes()).hexdigest()
            environment["LETOPIS_VEC1_EXTENSION"] = str(library)
            execute(["cargo", "test", "--locked", "--", "--nocapture"])
            example = ["cargo", "run", "--locked", "--example", "check", "--"]
            execute([*example, scratch / "supported.db", library])
            execute([*example, scratch / "missing.db", scratch / "absent"], rejection="vec1 extension")
            invalid = scratch / "invalid.so"
            invalid.write_text("not a native library")
            execute([*example, scratch / "invalid.db", invalid], rejection="vec1 extension")
            nofts = dict(environment, CARGO_TARGET_DIR=str(target) + "-nofts",
                         LIBSQLITE3_FLAGS="-USQLITE_ENABLE_FTS5")
            execute([*example, scratch / "nofts.db", library], env=nofts,
                    rejection="FTS5 contentless-delete is required")
            if args.system_sqlite:
                system = dict(environment, CARGO_TARGET_DIR=str(target) + "-system")
                execute(["cargo", "run", "--locked", "--no-default-features", "--example", "check", "--",
                         scratch / "old.db", library], env=system, rejection="SQLite 3.51.3+ required")
            execute(["cargo", "fmt", "--", "--check"])
            execute(["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"])
        manifest["result"] = "PASS"
    except Exception as error:
        manifest["error"] = str(error)
        raise
    finally:
        manifest["finished_utc"] = datetime.now(timezone.utc).isoformat()
        (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"TK-1 {manifest['result']}: {evidence}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        sys.exit(f"TK-1 failed: {error}")
