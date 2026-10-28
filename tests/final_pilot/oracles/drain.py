"""Independent C3 observations of the bounded concurrent embedding drain.

A loopback `ThreadingHTTPServer` accepts every request on its own thread and holds each
document request at a barrier, so two genuinely overlapping requests are counted while
a third stays unsent. Replies are literal vectors keyed by the document marker in the
request text. The private service probe (`service_probe.rs`, compiled in the copy)
drives the existing `wire_embedder` -> `Embedder::drain` and reads durable attempts and
vectors; T3 uses only the real binary and processes this oracle owns.
"""
from __future__ import annotations

import contextlib
import json
import os
import shutil
import signal
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

import copies
import evaluation
from evaluation import make_check, read_json
from oracles.http_fixture import BarrierServer, MALFORMED_MARKER, VECTORS

BOUND = 2
CHECK_IDS = {"C3-T1": ("two_requests_before_release", "bounded_buffer", "one_owner_short_writes"),
             "C3-T2": ("shared_hash_durable_attempts", "out_of_order_identity",
                       "invalid_retry_bound"),
             "C3-T3": ("detached_overlap", "caller_owner_lifecycle", "search_with_drain")}
RETRIES = 1
MAX_ATTEMPTS = 2
OVERLAP_WAIT_SECONDS = 10
QUIET_SECONDS = 1.0
DRAIN_WATCHDOG_SECONDS = 60
COMMAND_WATCHDOG_SECONDS = 20
PROBE_SOURCE = Path(__file__).resolve().parent / "service_probe.rs"
PROBE_EXAMPLE = "tk30_service_probe"
NOTE = '<!-- letopis: {{"created_at":1}} -->\nNote tk30-doc-{marker} body\n'
QUERY = b'{"query":"tk30 query","limit":5}'
# create_corpus names files MARKER-POSITION; beta is the second note of a change corpus.
CHANGED_FILE = "beta-1"


@dataclass
class Bench:
    """The disposable copy, its built binary and probe, and the evidence directory."""
    copy: Path
    binary: Path
    probe: Path
    evidence: Path


class ParticipantFailure(Exception):
    """The captured product could not publish or parse the literal corpus."""


def evaluate(copy: Path, task_id: str, evidence: Path) -> list[dict]:
    """Run `make check`, build the binary and the probe, then observe one C3 task."""
    return evaluation.observe_after_regression(copy, evidence, lambda: observe_task(
        Bench(copy, evaluation.build_binary(copy, evidence), build_probe(copy, evidence),
              evidence), task_id))


def observe_task(bench: Bench, task_id: str) -> list[dict]:
    """Observe one task; a product that cannot publish its corpus fails every check."""
    observe = {"C3-T1": observe_bounded, "C3-T2": observe_out_of_order,
               "C3-T3": observe_process}[task_id]
    try:
        return observe(bench)
    except ParticipantFailure as failure:
        return [make_check(check_id, False, f"product failure: {failure}")
                for check_id in CHECK_IDS[task_id]]


def observe_bounded(bench: Bench) -> list[dict]:
    """Two held requests, an independent short write and a busy lock, then completion."""
    with BarrierServer() as server:
        repo, hashes = create_corpus(bench, server, "bounded", ["alpha", "beta", "gamma"])
        with OwnedProcess([bench.probe, "drain", repo], env=make_runtime_env(bench)) as drain:
            window = observe_window(server)
            write = run_probe(bench, "write", repo)
            lock = run_probe(bench, "lock", repo)
            server.release_all()
            completed = drain.finish(DRAIN_WATCHDOG_SECONDS) == 0
        window.update(max_active=server.max_active, completed=completed)
        vectors = run_probe(bench, "observe", repo, *hashes.values())["value"]
    overlap, overlap_evidence = grade_overlap(window)
    identity = grade_identity(hashes, vectors)
    owner = (write["exit"] == 0 and write["value"] == {"committed": True}
             and lock["value"] == {"busy": True})
    return [
        make_check("two_requests_before_release", overlap and identity,
                   f"{overlap_evidence}; vectors match markers {identity}"),
        grade_buffer(window, bench.copy),
        make_check("one_owner_short_writes", overlap and owner,
                   f"{overlap_evidence}; write during inference {write['value']} "
                   f"in {write['elapsed']}s; lock {lock['value']}"),
    ]


def observe_out_of_order(bench: Bench) -> list[dict]:
    """Shared hashes, reverse replies, stale text/config and malformed retries."""
    shared = observe_shared(bench)
    changed_text = observe_change(bench, "text")
    changed_config = observe_change(bench, "config")
    removed = observe_change(bench, "removed")
    retry = observe_retry(bench)
    return [
        make_check("shared_hash_durable_attempts", grade_shared(shared), json.dumps(shared)),
        make_check("out_of_order_identity", shared["identity"] and changed_text["passed"]
                   and changed_config["passed"] and removed["passed"],
                   json.dumps({"reverse": shared["identity"], "text": changed_text,
                               "config": changed_config, "removed": removed})),
        make_check("invalid_retry_bound", grade_retry(retry), json.dumps(retry)),
    ]


def observe_process(bench: Bench) -> list[dict]:
    """Detached overlap, owner exclusion, foreground SIGINT, search and drain death."""
    with BarrierServer() as server:
        repo, _ = create_corpus(bench, server, "process", ["alpha", "beta", "gamma"])
        with OwnedProcess([bench.binary, "reindex"], cwd=repo, stdin=b"{}",
                          env=make_runtime_env(bench)) as foreground:
            launched = foreground.finish(COMMAND_WATCHDOG_SECONDS) == 0
            window = observe_window(server)
            pid = read_embed_pid(bench, repo)
            search = run_binary(bench, repo, ["search"], QUERY) if server.active else None
            lifecycle = observe_owner_rules(bench, server, repo, (foreground, pid))
        server.release_all()
        lifecycle["released_after_completion"] = wait_for_release(bench, repo)
        window.update(max_active=server.max_active,
                      completed=lifecycle["released_after_completion"])
    lifecycle["death"] = observe_death(bench)
    overlap, overlap_evidence = grade_overlap(window)
    detached = launched and pid is not None
    searched = search is not None and search["exit"] == 0 and read_ok(search)
    return [
        make_check("detached_overlap", overlap and detached,
                   f"{overlap_evidence}; detached drain pid {pid}"),
        make_check("caller_owner_lifecycle", all(lifecycle.values()), json.dumps(lifecycle)),
        make_check("search_with_drain", searched,
                   f"search during held drain: {describe(search)}"),
    ]


def grade_shared(observation: dict) -> bool:
    """Overlap reached, one hash never in flight twice, attempts durable while held."""
    return (observation["overlap"] and observation["max_same_hash"] == 1
            and len(observation["held_attempts"]) == BOUND
            and all(attempts is not None and attempts >= 1
                    for attempts in observation["held_attempts"].values()))


def grade_retry(observation: dict) -> bool:
    """A malformed reply during overlap stays recorded and stops at the attempt limit."""
    return (observation["overlap"] and observation["completed"]
            and observation["malformed_requests"] == MAX_ATTEMPTS
            and observation["attempts"] == MAX_ATTEMPTS and observation["error"]
            and observation["vector"] is None and observation["other_identity"])


def grade_buffer(window: dict, copy: Path) -> dict:
    """Observed bound plus the operator's source audit; overlap alone is never a pass."""
    overlap, overlap_evidence = grade_overlap(window)
    if not overlap:
        return make_check("bounded_buffer", False, overlap_evidence)
    audit, audit_evidence = evaluation.read_audit(copy, "bounded_buffer")
    return {"id": "bounded_buffer", "passed": audit,
            "evidence": f"{overlap_evidence}; {audit_evidence}"}


def observe_shared(bench: Bench) -> dict:
    with BarrierServer() as server:
        repo, hashes = create_corpus(bench, server, "shared",
                                     ["alpha", "alpha", "beta", "gamma"])
        with OwnedProcess([bench.probe, "drain", repo], env=make_runtime_env(bench)) as drain:
            window = observe_window(server)
            held = read_held_attempts(bench, repo, server, hashes)
            server.release_reversed()
            window["completed"] = drain.finish(DRAIN_WATCHDOG_SECONDS) == 0
        window["max_active"] = server.max_active
        vectors = run_probe(bench, "observe", repo, *hashes.values())["value"]
        return {"overlap": grade_overlap(window)[0], "max_same_hash": server.max_same,
                "held_attempts": held, "identity": grade_identity(hashes, vectors)}


def observe_change(bench: Bench, change: str) -> dict:
    """Change the text or the configuration of in-flight work; no stale vector survives."""
    with BarrierServer() as server:
        repo, hashes = create_corpus(bench, server, change, ["alpha", "beta"])
        with OwnedProcess([bench.probe, "drain", repo], env=make_runtime_env(bench)) as drain:
            window = observe_window(server)
            if change in ("text", "removed"):
                if change == "removed":
                    (repo / f"{CHANGED_FILE}.md").unlink()
                else:
                    write_note(repo, CHANGED_FILE, "changed")
                run_probe(bench, "refresh", repo)
            else:
                write_config(repo, server, model="fixture-changed")
                run_probe(bench, "sync", repo)
            server.release_reversed()
            window["completed"] = drain.finish(DRAIN_WATCHDOG_SECONDS) == 0
        window["max_active"] = server.max_active
        stale = run_probe(bench, "observe", repo, *hashes.values())["value"]
        fresh = read_hashes(bench, repo, [CHANGED_FILE], ["beta"]) if change == "text" else {}
        current = run_probe(bench, "observe", repo, fresh["beta"])["value"] if fresh else {}
    no_stale = grade_change(change, hashes, stale, current)
    return {"passed": grade_overlap(window)[0] and no_stale, "no_stale": no_stale,
            "window": window}


def grade_change(change: str, old: dict, stale: dict, fresh: dict) -> bool:
    """No stale vector survives an in-flight change; anything unobserved fails closed.

    `old` maps markers to the hashes before the change and `stale` is the probe's view of
    them afterwards. A config change leaves every old hash present with no vector; a text
    change leaves the old beta hash without a vector and `fresh` (the new beta hash) with
    the vector of the changed text.
    """
    if change == "removed":
        return has_no_vector(stale, old["beta"])
    if change == "text":
        current = [item for item in fresh.values() if isinstance(item, dict)]
        return (has_no_vector(stale, old["beta"]) and len(current) == 1
                and current[0].get("vector") == VECTORS["changed"])
    return bool(old) and all(has_no_vector(stale, digest) for digest in old.values())


def has_no_vector(observed: dict, digest: str) -> bool:
    item = observed.get(digest)
    return isinstance(item, dict) and "vector" in item and item["vector"] is None


def observe_retry(bench: Bench) -> dict:
    with BarrierServer() as server:
        repo, hashes = create_corpus(bench, server, "retry", ["alpha", MALFORMED_MARKER],
                                     retries=RETRIES)
        with OwnedProcess([bench.probe, "drain", repo], env=make_runtime_env(bench)) as drain:
            window = observe_window(server)
            server.release_reversed()
            window["completed"] = drain.finish(DRAIN_WATCHDOG_SECONDS) == 0
        window["max_active"] = server.max_active
        observed = run_probe(bench, "observe", repo, *hashes.values())["value"]
        malformed = read_item(observed, hashes[MALFORMED_MARKER])
        return {"overlap": grade_overlap(window)[0], "completed": window["completed"],
                "malformed_requests": server.count(MALFORMED_MARKER),
                "attempts": malformed.get("attempts"), "error": bool(malformed.get("error")),
                "vector": malformed.get("vector"),
                "other_identity": grade_identity({"alpha": hashes["alpha"]}, observed)}


def grade_identity(hashes: dict, observed: dict) -> bool:
    """Every document hash stores exactly the literal vector of its own marker."""
    return all(isinstance(observed.get(digest), dict)
               and observed[digest].get("vector") == VECTORS[marker]
               for marker, digest in hashes.items())


def grade_overlap(window: dict) -> tuple[bool, str]:
    """Exactly BOUND requests held before the first release, never more, then completion."""
    passed = (window["held_before_release"] == BOUND
              and window["requests_before_release"] == BOUND
              and window["max_active"] <= BOUND and window["completed"])
    return passed, (f"held before release {window['held_before_release']}, requests "
                    f"{window['requests_before_release']}, max active {window['max_active']}, "
                    f"completed {window['completed']}")


def observe_owner_rules(bench: Bench, server: "BarrierServer", repo: Path,
                        owner: tuple) -> dict:
    """While the drain holds requests: a second owner stops, SIGINT ends only its caller.

    `owner` is the launching foreground (in its own session) and the drain pid from the
    embed lock. SIGINT goes to our own sessions only: the launcher's (a drain left in it
    would not be detached) and a blocked foreground search's.
    """
    launcher, pid = owner
    requests = len(server.requests)
    second = run_binary(bench, repo, ["embed", "--drain"], b"")
    rules = {"second_owner_stopped": second["exit"] == 0 and len(server.requests) == requests}
    launcher.interrupt()
    with OwnedProcess([bench.binary, "search"], cwd=repo, env=make_runtime_env(bench)) as search:
        time.sleep(QUIET_SECONDS)
        search.interrupt()
        # Only the end of the interrupted caller is required; its JSON envelope is TK-29's.
        rules["sigint_ends_caller"] = search.finish(COMMAND_WATCHDOG_SECONDS) is not None
    time.sleep(QUIET_SECONDS)
    rules["drain_survives_sigint"] = pid is not None and is_alive(pid) and server.active > 0
    return rules


def observe_death(bench: Bench) -> bool:
    """Our own drain dies while held: the lock is released and attempts stay recorded."""
    with BarrierServer() as server:
        repo, hashes = create_corpus(bench, server, "death", ["alpha", "beta"])
        with OwnedProcess([bench.binary, "embed", "--drain"], cwd=repo, stdin=b"",
                          env=make_runtime_env(bench)) as drain:
            held = server.wait_for_held(1, OVERLAP_WAIT_SECONDS)
            drain.kill()
        released = is_released(bench, repo)
        attempts = read_held_attempts(bench, repo, server, hashes)
    return held and released and bool(attempts) and all(
        value is not None and value >= 1 for value in attempts.values())


def observe_window(server: "BarrierServer") -> dict:
    """Wait for BOUND held requests, then a quiet period in which no third may arrive."""
    server.wait_for_held(BOUND, OVERLAP_WAIT_SECONDS)
    time.sleep(QUIET_SECONDS)
    return {"held_before_release": server.active,
            "requests_before_release": len(server.requests)}


def read_held_attempts(bench: Bench, repo: Path, server: "BarrierServer",
                       hashes: dict) -> dict:
    held = [marker for marker in hashes if server.count_active(marker)]
    if not held:
        return {}
    observed = run_probe(bench, "observe", repo, *[hashes[m] for m in held])["value"]
    return {marker: read_item(observed, hashes[marker]).get("attempts") for marker in held}


def create_corpus(bench: Bench, server: "BarrierServer", name: str, markers: list[str],
                  retries: int = 0) -> tuple[Path, dict]:
    """Publish literal notes with the real binary, then point the config at the server.

    Returns the repository and `{marker: search hash}`; repeated markers share a hash.
    """
    repo = bench.copy / "drain" / name
    repo.mkdir(parents=True)
    (repo / ".gitignore").write_text(".letopis/\n")
    files = [f"{marker}-{number}" for number, marker in enumerate(markers)]
    for file, marker in zip(files, markers):
        write_note(repo, file, marker)
    initialize_git(repo)
    published = run_binary(bench, repo, ["reindex"], b"{}")
    if published["exit"] != 0:
        raise ParticipantFailure(f"reindex of the literal corpus failed: {describe(published)}")
    write_config(repo, server, retries=retries)
    return repo, read_hashes(bench, repo, files, markers)


def read_hashes(bench: Bench, repo: Path, files: list[str],
                markers: list[str] | None = None) -> dict:
    result = run_probe(bench, "hashes", repo, *[f"{file}.md" for file in files])
    hashes = {marker: result["value"].get(f"{file}.md")
              for file, marker in zip(files, markers or files)}
    if not all(isinstance(digest, str) for digest in hashes.values()):
        raise ParticipantFailure(f"probe hashes of the literal notes failed: {result['problem']}")
    return hashes


def write_note(repo: Path, file: str, marker: str) -> None:
    (repo / f"{file}.md").write_text(NOTE.format(marker=marker))


def write_config(repo: Path, server: "BarrierServer", retries: int = 0,
                 model: str = "fixture") -> None:
    (repo / ".letopis").mkdir(exist_ok=True)
    (repo / ".letopis/config.json").write_text(json.dumps(
        {"embedding_model": {
            "enabled": True, "provider": "ollama", "model": model,
            "parameters": {"base_url": server.url}, "max_retries": retries,
        }}))


def initialize_git(repo: Path) -> None:
    for argv in (["init", "-q"], ["add", "."],
                 ["-c", "user.name=Oracle", "-c", "user.email=oracle@invalid.local",
                  "commit", "-qm", "corpus"]):
        subprocess.run(["git", "-C", str(repo), *argv], check=True, capture_output=True,
                       env=copies.GIT_ENVIRONMENT,
                       timeout=evaluation.GIT_TIMEOUT_SECONDS)


def build_probe(copy: Path, evidence: Path) -> Path:
    """Compile the private probe as an example of the copy; failure is infra, not RED."""
    examples = copy / "repo/examples"
    examples.mkdir(exist_ok=True)
    shutil.copyfile(PROBE_SOURCE, examples / f"{PROBE_EXAMPLE}.rs")
    result = evaluation.run_observed(
        ["cargo", "build", "--locked", "--offline", "--example", PROBE_EXAMPLE],
        cwd=copy / "repo", env=evaluation.build_environment(copy), evidence=evidence,
        label="cargo build service probe", timeout=evaluation.BUILD_TIMEOUT_SECONDS)
    probe = copy / "target/debug/examples" / PROBE_EXAMPLE
    if result["exit"] != 0 or not probe.is_file():
        evaluation.save_tail(evidence, "service-probe-build.log", result)
        raise evaluation.InfraError("service probe does not compile against the captured source",
                                    evaluation.CAUSE_PROBE)
    return probe


def run_probe(bench: Bench, mode: str, repo: Path, *items: str) -> dict:
    result = evaluation.run_observed([str(bench.probe), mode, str(repo), *items], cwd=repo,
                                     env=make_runtime_env(bench), evidence=bench.evidence,
                                     label=f"probe {mode}", timeout=COMMAND_WATCHDOG_SECONDS)
    value = read_json(result["stdout"])
    ok = result["exit"] == 0 and isinstance(value, dict)
    result["value"] = value if ok else {}
    result["problem"] = None if ok else (
        f"exit {result['exit']}, output {result['stdout'][:200]!r}")
    return result


def wait_for_release(bench: Bench, repo: Path) -> bool:
    """True once an observed `status` lists no embed lock; unobserved never counts."""
    deadline = time.monotonic() + DRAIN_WATCHDOG_SECONDS
    while time.monotonic() < deadline:
        if is_released(bench, repo):
            return True
        time.sleep(0.2)
    return False


def is_released(bench: Bench, repo: Path) -> bool:
    return observe_embed_lock(bench, repo) == (True, None)


def read_embed_pid(bench: Bench, repo: Path):
    return observe_embed_lock(bench, repo)[1]


def observe_embed_lock(bench: Bench, repo: Path) -> tuple[bool, int | None]:
    """Return (observed, embed lock pid) from `status`.

    Observed only when `status` exits 0 with an `ok: true` object listing its locks; any
    other output is unobserved, so it can never be mistaken for a released lock.
    """
    result = run_binary(bench, repo, ["status"], b"{}")
    status = read_json(result["stdout"])
    body = status.get("result") if isinstance(status, dict) and status.get("ok") is True else None
    locks = body.get("locks") if isinstance(body, dict) else None
    if result["exit"] != 0 or not isinstance(locks, list):
        return False, None
    pids = [lock.get("pid") for lock in locks
            if isinstance(lock, dict) and lock.get("name") == "embed"]
    return True, pids[0] if pids else None


def run_binary(bench: Bench, repo: Path, argv: list[str], stdin: bytes) -> dict:
    return evaluation.run_observed([str(bench.binary), *argv], cwd=repo,
                                   env=make_runtime_env(bench), evidence=bench.evidence,
                                   label=f"letopis {' '.join(argv)}", stdin=stdin,
                                   timeout=COMMAND_WATCHDOG_SECONDS)


def make_runtime_env(bench: Bench) -> dict:
    return {"PATH": os.environ.get("PATH", ""), "HOME": str(bench.copy / "home"),
            "LC_ALL": "C", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}


def read_item(observed: dict, digest: str) -> dict:
    """The probe's record of one hash, or an empty record when it is absent or malformed."""
    item = observed.get(digest)
    return item if isinstance(item, dict) else {}


def is_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def read_ok(result: dict) -> bool:
    value = read_json(result["stdout"])
    return isinstance(value, dict) and value.get("ok") is True


def describe(result: dict | None) -> str:
    if result is None:
        return "not run: no request was held"
    return f"exit {result['exit']} in {result['elapsed']}s"


class OwnedProcess:
    """A child this oracle started in its own session; only that session is signalled.

    With `stdin` the bytes are written and the pipe closed; without it stdin stays open.
    `env` is the complete child environment; nothing is inherited from the operator.
    """

    def __init__(self, argv: list, *, env: dict, cwd: Path | None = None,
                 stdin: bytes | None = None):
        self.child = subprocess.Popen([str(item) for item in argv], cwd=cwd, env=env,
                                      stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                      stderr=subprocess.DEVNULL, start_new_session=True)
        if stdin is not None:
            self.child.stdin.write(stdin)
            self.child.stdin.close()

    def __enter__(self) -> "OwnedProcess":
        return self

    def __exit__(self, *_exc) -> None:
        if self.child.poll() is None:
            self.kill()
        if not self.child.stdin.closed:
            self.child.stdin.close()

    def finish(self, timeout: float) -> int | None:
        """Wait for the exit code; at the watchdog kill our own child and return None."""
        try:
            return self.child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.kill()
            return None

    def interrupt(self) -> None:
        """Send SIGINT to this child's own process group, as a terminal Ctrl-C would."""
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.child.pid, signal.SIGINT)

    def kill(self) -> None:
        self.child.kill()
        self.child.wait()
