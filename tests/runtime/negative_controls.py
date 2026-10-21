#!/usr/bin/env python3
"""Compile known-bad variants in disposable copies, require invariant failures."""
import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parent


def copy_crates(destination):
    for name in ("storage-build", "publication", "embedding", "runtime"):
        shutil.copytree(ROOT.parent / name, destination / "tests" / name,
                        ignore=shutil.ignore_patterns("target", ".build", "__pycache__"))
    return destination / "tests/runtime"


def run_command(argv, *, cwd, env):
    # Inherit the outer mutation command group. The full runner owns its 600s
    # timeout and kills this WHOLE group on interruption, including nested Cargo.
    return subprocess.run(argv, cwd=cwd, env=env, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def interruption_probe(path, env, work):
    control = env.get("TK4_MUTATION_CONTROL")
    if not control:
        return
    # Only the regression sets this fixture channel. This command uses the SAME
    # execution primitive as Cargo, so a nested-session regression is observable.
    code = """import json,os,signal,socket,sys
s=socket.socket(socket.AF_UNIX);s.connect(sys.argv[1])
s.sendall((json.dumps({'path':sys.argv[2],'group':os.getpgrp(),'parent_group':os.getpgid(os.getppid())})+'\\n').encode())
if s.recv(1)==b'K': os.kill(os.getpid(),signal.SIGKILL)
"""
    run_command([sys.executable, "-c", code, control, str(path)], cwd=work, env=env)


def check_mutations(work, evidence, env):
    launch = work / "src/launch.rs"
    support = work / "tests/support/mod.rs"
    cancellation = work / "tests/cancellation.rs"
    cases = [
        ("unguarded-drain", launch, "let Some(lease) = try_lock(root)? else", 'let Some(lease) = Some(File::open(root.join(".letopis/embed.lock"))?) else',
         "contention", "ten_probe_winners_admit_one_drain", "assertion failed: held"),
        ("pid-file-ownership", launch, "    loop {\n        // SAFETY: file owns", "    if file.metadata()?.len() > 0 { return Ok(None); }\n    loop {\n        // SAFETY: file owns",
         "contention", "dead_owners_and_stale_pids_do_not_block", "PID contents blocked free kernel lease"),
        ("unlink-held-file", launch, "    // PID is diagnostic only", '    std::fs::remove_file(root.join(".letopis/embed.lock"))?;\n    // PID is diagnostic only',
         "contention", "ten_probe_winners_admit_one_drain", ("assertion failed: held", "second active drain")),
        ("group-signal", support, "libc::kill(self.pid() as i32, signal)", "libc::kill(-libc::getpgid(self.pid() as i32), signal)",
         "cancellation", "sigint_waiter_leaves_owner_and_drain_alive", "SIGINT waiter killed foreign publication owner"),
        ("inherited-session", launch, "if libc::setsid() == -1", "if false",
         "contention", "launcher_exit_does_not_wait_or_leak_locks", "assertion `left != right` failed"),
        ("inherited-stdout", launch, ".stdout(Stdio::null())", ".stdout(Stdio::inherit())",
         "contention", "launcher_exit_does_not_wait_or_leak_locks", "assertion `left == right` failed"),
        ("pre-flock-pause", cancellation, "waiting.resume()?; // enter the real blocking flock, with no later fixture hook", "let _waiting = waiting; // deliberately remain before the blocking syscall",
         "cancellation", "sigint_inside_blocking_flock_leaves_other_processes_alive", "waiter never entered blocking flock"),
    ]
    manifest = {"result": "FAIL", "cases": [], "disposable_workspace": str(work),
                "build_target": env["CARGO_TARGET_DIR"], "nested_commands_inherit_group": True}
    try:
        for name, path, before, after, suite, test, invariant in cases:
            original = path.read_bytes()
            text = original.decode()
            if text.count(before) != 1:
                raise RuntimeError(f"{name}: mutation anchor is not unique")
            entry = {"name": name, "file": str(path.relative_to(work)), "original_sha256": hashlib.sha256(original).hexdigest(), "test": test}
            manifest["cases"].append(entry)
            try:
                path.write_text(text.replace(before, after))
                entry["mutated_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                interruption_probe(path, env, work)
                compile_argv = ["cargo", "test", "--locked", "--test", suite, "--no-run"]
                compiled = run_command(compile_argv, cwd=work, env=env)
                (evidence / f"{name}-compile.txt").write_text(compiled.stdout + compiled.stderr)
                entry.update(compile_argv=compile_argv, compile_exit=compiled.returncode)
                if compiled.returncode:
                    raise RuntimeError(f"{name}: compile failure is not an invariant failure")
                argv = ["cargo", "test", "--locked", "--test", suite, test, "--", "--exact", "--nocapture"]
                result = run_command(argv, cwd=work, env=env)
                log = result.stdout + result.stderr
                (evidence / f"{name}.txt").write_text(log)
                # Unlink can expose a free inode or admit a second owner first.
                # Both are exact exclusion violations; unrelated failures still fail closed.
                accepted = (invariant,) if isinstance(invariant, str) else invariant
                detected = next((message for message in accepted if message in log), None)
                entry.update(argv=argv, exit_code=result.returncode,
                             invariant=detected, accepted_invariants=list(accepted))
                if result.returncode == 0 or detected is None or "test result: FAILED" not in log:
                    raise RuntimeError(f"{name}: expected assertion was not detected")
                entry["result"] = "DETECTED"
                print(f"{name}: DETECTED ({test})", flush=True)
            finally:
                path.write_bytes(original)
                entry["restored_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                assert path.read_bytes() == original
        # Leave both source and compiled binaries in a verified unmutated state.
        result = run_command(["cargo", "test", "--locked", "--", "--nocapture"], cwd=work, env=env)
        (evidence / "restored-suite.txt").write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError("restored suite failed")
        manifest["restored_suite_exit"] = result.returncode
        manifest["result"] = "PASS"
    finally:
        (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    # SIGKILL may bypass finally. Mutate only a disposable source copy, never the
    # reviewed/untracked worktree; its own build output cannot overwrite our bins.
    with tempfile.TemporaryDirectory(prefix="tk4-mutants-") as scratch:
        work = copy_crates(Path(scratch))
        env = dict(os.environ, CARGO_TARGET_DIR=str(Path(scratch) / "target"))
        check_mutations(work, evidence, env)


if __name__ == "__main__":
    main()
