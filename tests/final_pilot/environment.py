"""TK-30 credential-free build context and model-free local environment preflight.

Nothing here reads credentials, downloads, builds or pulls an image, or starts an agent.
Asset verification, binary extraction and the image recipe are adapted from the pinned
TK-17 `prepare_image.py`, and the empty-memory probe from its `records.initialize_memory`
(see TK17_SOURCES); its downloader, hardcoded base/study/order, session runner and auth
copy are not carried over. Local checks never clear model or embedding access: those are
only observable in a later, separately authorized paid run.
"""
from __future__ import annotations

import json
import os
import shlex
import shutil
import socket
import subprocess
import tarfile
import tempfile
from pathlib import Path

import copies
import study

# Recorded release digests of the Codex 0.164.0 linux/arm64 musl archives (TK-17 pins).
PINNED_ASSETS = (
    ("codex-linux.tar.gz", "codex-aarch64-unknown-linux-musl",
     "29a507a2a3fd8f6cd94b96b762d281d299d15d7902dee6132d314b78cf9108eb"),
    ("code-mode-host.tar.gz", "codex-code-mode-host-aarch64-unknown-linux-musl",
     "0e0bdde24d5b31b05519693635baa9c4f2dc4fe7eee31c67b92155f190dd32d2"),
)
TK17_SOURCES = {
    "tests/early_pilot/prepare_image.py":
        "da9013ac13542b525c6640b3ded17a9072eb7beebe88e72ec572d49dd6bdc3f7",
    "tests/early_pilot/records.py":
        "3421de72e4e979a0760cf07111f14aa49c666354baf0057a6b72e7b12b682805",
    "tests/early_pilot/test_image.py":
        "eda666022fca2083a4350305eb06f33c5920f305de93d15e5dc9c36518e15606",
}
PLATFORM = "linux/arm64"
VERSION_LINE = f"codex-cli {study.AGENT_VERSION}"
MEMORY_BINARY = "letopis"
REQUIRED_TOOLS = ("rustc", "cargo", "rustfmt", "git", "rg", "python3", "codex", MEMORY_BINARY)
# Repositories with history, agent/memory state and credentials never enter a context.
FORBIDDEN_SOURCE_NAMES = frozenset({".git", ".codex", ".engram", ".letopis", "auth.json"})
# No local probe of this preflight can establish these: Git/target separation and
# participant isolation belong to the future participant run command (TK-31), and model
# and embedding access need the separately authorized paid run.
ALWAYS_PENDING = ("preflight:git_target_separation", "participant_isolation", "model_access",
                  "embedding_access")
CONTAINER_SCOPE = "preflight container only; not the participant run: "
DOCKER_ENV_KEYS = ("PATH", "HOME", "DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG")
DOCKER_RUN = ("docker", "run", "--rm", "--pull", "never", "--network", "none",
              "--cap-drop", "ALL", "--entrypoint", "bash")
CHECK_MARKER = "# tk30-check:"
DOCKER_TIMEOUT_SECONDS = 300
EGRESS_TIMEOUT_SECONDS = 3
# TEST-NET-1 (RFC 5737): never routed, used only to ask the kernel for the default route.
ROUTE_PROBE_ADDRESS = "192.0.2.1"
EGRESS_PROBE = """python3 -c '
import socket
names = [row.split(":")[0].strip() for row in open("/proc/net/dev").readlines()[2:]]
try:
    socket.create_connection(("{host}", {port}), timeout={timeout})
except OSError:
    print("isolated" if names == ["lo"] else "interfaces:" + ",".join(names))
else:
    print("egress:open")'"""
DETAIL_LIMIT = 500
EMPTY_MEMORY = {"knowledge_nodes": 0}
# The disposable store gets no provider configuration, so initializing it cannot drain.
MEMORY_SCRIPT = f"""set -e
workspace=$(mktemp -d)
cd "$workspace"
git init -q
{MEMORY_BINARY} reindex <<< '{{}}' > /dev/null
{MEMORY_BINARY} status <<< '{{}}' > /dev/null
python3 - <<'PY'
import json, sqlite3
db = sqlite3.connect("file:.letopis/cache.db?mode=ro", uri=True)
query = "select count(*) from nodes where kind in ('vertex', 'metavertex')"
print(json.dumps({{"knowledge_nodes": db.execute(query).fetchone()[0]}}))
PY"""
DOCKERFILE = """ARG STACK_IMAGE
FROM ${STACK_IMAGE}
RUN rm -rf /repo /root/.codex /root/.engram /root/.cache /tmp/*
RUN apt-get update && apt-get install -y --no-install-recommends ripgrep && rm -rf /var/lib/apt/lists/*
RUN printf 'export PATH=/usr/local/cargo/bin:$PATH\\n' > /etc/profile.d/letopis-cargo.sh
COPY codex-aarch64-unknown-linux-musl /usr/local/bin/codex
COPY codex-code-mode-host-aarch64-unknown-linux-musl /usr/local/bin/codex-code-mode-host
COPY source /repo
RUN cd /repo && cargo fetch --locked && cargo build --locked --offline --release && install -m 0555 target/release/letopis /usr/local/bin/letopis && rm -rf /repo
ENV PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin
WORKDIR /workspace
"""


def prepare_context(source: Path, assets: Path, output: Path) -> dict:
    """Create a new credential-free build context from the source and supplied assets.

    Missing assets return `pending` naming each absent file and create nothing; nothing is
    downloaded. The source must not contain Git, agent/memory state or `auth.json`.
    Writes `source/`, the extracted binaries, `Dockerfile` and `provenance.json`; the
    image is neither built nor run.

    Raises ValueError when an asset differs from its pinned digest or the source leaks
    forbidden state, and FileExistsError when output exists. A failure after the output
    directory was created removes it again.
    """
    missing = [name for name, _, _ in PINNED_ASSETS if not (assets / name).is_file()]
    if missing:
        return {"context": None, "pending": [f"asset:{name}" for name in missing],
                "model_executions": 0}
    for name, _, expected in PINNED_ASSETS:
        verify_asset(assets / name, expected)
    files = copies.walk_files(source, ())
    reject_leaks(relative for relative, _ in files)
    inventory = copies.snapshot(source, ())
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    try:
        return write_context(assets, files, inventory, output)
    except BaseException:
        shutil.rmtree(output, ignore_errors=True)
        raise


def write_context(assets: Path, files: list, inventory: dict, output: Path) -> dict:
    """Write the build context into the new output directory and return its result."""
    for relative, path in files:
        target = output / "source" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    for name, binary, _ in PINNED_ASSETS:
        extract_binary(assets / name, binary, output / binary)
    (output / "Dockerfile").write_text(DOCKERFILE, encoding="utf-8")
    provenance = {
        "codex_version": study.AGENT_VERSION, "platform": PLATFORM,
        "asset_sha256": {name: digest for name, _, digest in PINNED_ASSETS},
        "source": {"items": inventory["items"], "sha256": inventory["sha256"]},
        "tk17_sources": {"commit": copies.SOURCE_RECEIPTS["commit"], "files": TK17_SOURCES},
        "model_executions": 0}
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n",
                                            encoding="utf-8")
    return {"context": str(output), "pending": [], **provenance}


def preflight(root: Path, image: str) -> dict:
    """Run local, model-free probes of an already-local image; return checks and pending.

    Pending always starts from the manifest readiness (null model, product, G-2 or config
    stay pending), adds every check that did not pass and always ends with ALWAYS_PENDING:
    Git/target separation, participant isolation and model and embedding access, which no
    local probe can establish. `container_isolation` covers only this preflight's own
    container, never the participant run. A stale or absent image is not
    run; an image without a pinned digest is probed but its digest stays pending.
    Containers run with `--pull never`, no network, mounts, environment or capabilities.
    """
    manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    pending = study.assess_readiness(manifest)["pending"]
    pinned = (manifest.get("environment") or {}).get("image")
    image_id = inspect_image(image)
    checks = {"image_digest": check_digest(image_id, pinned)}
    if image_id is not None and checks["image_digest"]["passed"] is not False:
        checks["tools"] = check_tools(image_id)
        checks["memory"] = check_memory(image_id)
        checks["container_isolation"] = check_isolation(image_id, root)
    pending += [f"preflight:{name}" for name, check in checks.items()
                if check["passed"] is not True]
    return {"checks": checks, "pending": [*pending, *ALWAYS_PENDING], "model_executions": 0}


def verify_asset(path: Path, expected: str) -> None:
    """Raise ValueError unless the file's SHA-256 equals the pinned digest."""
    if copies.hash_file(path) != expected:
        raise ValueError("asset differs from recorded release digest: " + path.name)


def reject_leaks(relatives) -> None:
    """Raise ValueError when any path component is Git, agent/memory state or auth."""
    for relative in relatives:
        if FORBIDDEN_SOURCE_NAMES.intersection(relative.split("/")):
            raise ValueError(f"context source must not contain {relative}")


def extract_binary(package: Path, binary: str, output: Path) -> None:
    with tarfile.open(package, "r:gz") as archive:
        member = archive.getmember(binary)
        if not member.isfile():
            raise ValueError("release member is not a regular file")
        with archive.extractfile(member) as source, output.open("xb") as target:
            shutil.copyfileobj(source, target)
    output.chmod(0o755)


def inspect_image(image: str) -> str | None:
    """Return the local image ID, or None when the image is not present locally."""
    result = subprocess.run(["docker", "image", "inspect", "--format", "{{.Id}}", image],
                            capture_output=True, text=True, env=build_docker_env(),
                            timeout=DOCKER_TIMEOUT_SECONDS)
    image_id = result.stdout.strip()
    return image_id if result.returncode == 0 and image_id else None


def check_digest(image_id: str | None, pinned) -> dict:
    if image_id is None:
        return {"passed": None, "detail": "image is not present locally; nothing is pulled"}
    if not pinned:
        return {"passed": None, "detail": f"no pinned digest in manifest; observed {image_id}"}
    if image_id != pinned:
        return {"passed": False, "detail": f"stale image {image_id}; pinned {pinned}"}
    return {"passed": True, "detail": image_id}


def check_tools(image_id: str) -> dict:
    """Login-shell PATH has every tool, the memory binary is outside the workspace and
    `codex --version` prints exactly the pinned version."""
    script = "\n".join([
        f"for tool in {' '.join(REQUIRED_TOOLS)}; do",
        '  command -v "$tool" > /dev/null || echo "missing:$tool"',
        "done",
        f'case "$(readlink -f "$(command -v {MEMORY_BINARY})")" in',
        f"  /workspace/*) echo shared:{MEMORY_BINARY};;",
        "esac",
        "codex --version"])
    result = run_check(image_id, "tools", script)
    return describe(result, result.stdout.splitlines() == [VERSION_LINE])


def check_memory(image_id: str) -> dict:
    """A disposable store initialized without provider config is empty by CLI and SQL."""
    result = run_check(image_id, "memory", MEMORY_SCRIPT)
    lines = result.stdout.splitlines()
    try:
        observed = json.loads(lines[-1]) if lines else None
    except ValueError:
        observed = None
    return describe(result, observed == EMPTY_MEMORY)


def check_isolation(image_id: str, root: Path) -> dict:
    """This preflight container sees no Docker socket, operator or peer canary path, has
    only a loopback interface and no route to a toy port on this host's address; it says
    nothing about the participant run command."""
    host = find_host_address()
    with tempfile.TemporaryDirectory() as folder, socket.socket() as listener:
        canary = Path(folder) / "operator-canary"
        canary.write_text("operator only\n", encoding="utf-8")
        listener.bind((host, 0))
        listener.listen()
        port = listener.getsockname()[1]
        paths = " ".join(shlex.quote(str(path)) for path in (canary, root.resolve()))
        script = "\n".join([
            "test ! -S /var/run/docker.sock || echo docker-socket",
            f'for path in {paths}; do test ! -e "$path" || echo "visible:$path"; done',
            EGRESS_PROBE.format(host=host, port=port, timeout=EGRESS_TIMEOUT_SECONDS)])
        result = run_check(image_id, "container_isolation", script)
        reached = has_pending_connection(listener)
    check = describe(result, result.stdout.splitlines() == ["isolated"] and not reached)
    return {**check, "detail": CONTAINER_SCOPE + check["detail"]}


def run_check(image_id: str, name: str, script: str) -> subprocess.CompletedProcess:
    argv = [*DOCKER_RUN, image_id, "-lc", f"{CHECK_MARKER}{name}\n{script}"]
    return subprocess.run(argv, capture_output=True, text=True, env=build_docker_env(),
                          timeout=DOCKER_TIMEOUT_SECONDS)


def build_docker_env() -> dict:
    """Pass only what the Docker CLI needs to find its daemon; no tokens or agent homes."""
    return {key: os.environ[key] for key in DOCKER_ENV_KEYS if key in os.environ}


def describe(result: subprocess.CompletedProcess, observed_ok: bool) -> dict:
    output = (result.stdout + result.stderr)[-DETAIL_LIMIT:]
    return {"passed": result.returncode == 0 and observed_ok, "detail": output}


def has_pending_connection(listener: socket.socket) -> bool:
    listener.setblocking(False)
    try:
        connection, _ = listener.accept()
    except BlockingIOError:
        return False
    connection.close()
    return True


def find_host_address() -> str:
    """Address of this host's default route, so the probe tests a connection, not a name.

    Connecting a UDP socket only selects the route; no packet is sent. Without any route
    the loopback address is used and the interface-list check still guards the result.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        try:
            probe.connect((ROUTE_PROBE_ADDRESS, 9))
        except OSError:
            return "127.0.0.1"
        return probe.getsockname()[0]
