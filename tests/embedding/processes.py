"""Bounded POSIX command groups for the macOS/Linux scratch runner."""
import os
import signal
import subprocess


def run_command(argv, *, cwd, env, timeout=600):
    process = subprocess.Popen(argv, cwd=cwd, env=env, text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except BaseException as error:
        # Cargo and nested runners spawn descendants. Kill their command group
        # before draining pipes, including when the caller receives Ctrl-C.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = process.communicate()
        if isinstance(error, subprocess.TimeoutExpired):
            raise subprocess.TimeoutExpired(argv, timeout, output=stdout, stderr=stderr) from error
        raise
    return subprocess.CompletedProcess(argv, process.returncode, stdout, stderr)
