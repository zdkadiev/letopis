"""Deterministic isolated inputs and independently pinned release artifacts."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
GENERATOR = ROOT / "tests/target_corpus/generate.py"
SCENARIOS = ("search", "inspect", "traverse", "check", "status", "refresh_one",
             "refresh_several", "refresh_bulk", "reindex")
FIRST_ID = "knowledge/part-0000.md#node-000000"
PRODUCT_PREFIXES = ("src/", "vendor/", "native/", "vendored/")
PRODUCT_FILES = {"Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"}


def prepare_fixture(output: Path, nodes: int) -> dict:
    """Generate new, model-free input; no product process is invoked."""
    verify_new_output(output)
    specification = importlib.util.spec_from_file_location("performance_generator", GENERATOR)
    module = importlib.util.module_from_spec(specification)
    sys.modules[specification.name] = module
    specification.loader.exec_module(module)
    manifest = module.generate_corpus(
        output, module.CorpusOptions(seed=65, knowledge_count=nodes,
                                     fragment_bytes=2000, elements_per_file=30))
    repository = output / "repository"
    service = repository / ".letopis"
    service.mkdir(mode=0o700)
    write_json(service / "config.json", {"embedding_model": {
        "enabled": False, "provider": "ollama", "model": "performance-fixture",
        "parameters": {"base_url": "http://127.0.0.1:1/api"}, "max_retries": 0}})
    (repository / ".gitignore").write_text(".letopis/\n", encoding="utf-8")
    return dict(manifest, repository=repository)


def prepare_existing_fixture(manifest_path: Path, output: Path) -> dict:
    """Copy verified generated inputs and idle projection; never measure in the original."""
    verify_new_output(output, manifest_path.parent)
    specification = importlib.util.spec_from_file_location("performance_validator", GENERATOR.with_name("validate.py"))
    module = importlib.util.module_from_spec(specification)
    sys.modules[specification.name] = module
    specification.loader.exec_module(module)
    manifest = module.validate_corpus(manifest_path)
    repository = manifest_path.parent / "repository"
    config = json.loads((repository / ".letopis/config.json").read_text())
    if manifest["options"]["seed"] != 65 or config["embedding_model"]["enabled"] is not False:
        raise ValueError("only disabled synthetic issue-65 fixtures may be reused")
    if not (repository / ".git").is_dir() or (repository / ".git").is_symlink():
        raise ValueError("standalone generated repository required")
    output.mkdir(parents=True)
    shutil.copytree(repository, output / "repository")
    write_json(output / "corpus.json", manifest)
    return dict(manifest, repository=output / "repository", reused_manifest=str(manifest_path))


def build_release(source: Path, target: Path, evidence: Path) -> dict:
    """Build a locked release from clean product inputs and pin its provenance."""
    before = read_product_inputs(source)
    command = ["cargo", "build", "--release", "--locked", "--offline", "--message-format=json"]
    environment = get_environment()
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_PROFILE_RELEASE_DEBUG="1")
    evidence.mkdir(parents=True, exist_ok=True)
    with (evidence / "cargo-build.log").open("wb") as errors:
        result = subprocess.run(command, cwd=source, env=environment, stdout=subprocess.PIPE,
                                stderr=errors, timeout=600, check=True)
    after = read_product_inputs(source)
    if before != after:
        raise ValueError("product inputs changed during build")
    messages = [json.loads(line) for line in result.stdout.splitlines()]
    artifact = next(item for item in reversed(messages)
                    if item.get("reason") == "compiler-artifact"
                    and item.get("target", {}).get("name") == "letopis"
                    and item.get("executable"))
    artifact_path = Path(artifact["executable"]).resolve()
    product = evidence / "product"
    product.mkdir()
    binary = product / "letopis"
    shutil.copy2(artifact_path, binary)
    symbols = artifact_path.with_name(artifact_path.name + ".dSYM")
    if symbols.exists():
        shutil.copytree(symbols, product / "letopis.dSYM")
    pin = {"executable": str(binary), "sha256": hash_file(binary), "inputs": before,
           "product_commit": run_git(source, "rev-parse", "HEAD").decode().strip(),
           "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
           "command": command, "release_debug": "1", "target": str(target)}
    write_json(evidence / "build.json", pin)
    return pin


def prepare_scenario(fixture: dict, name: str, output: Path) -> dict:
    """Copy equivalent input state and apply a single deterministic edit recipe."""
    if name not in SCENARIOS:
        raise ValueError("unknown scenario")
    verify_new_output(output, fixture["repository"])
    started = time.monotonic()
    shutil.copytree(fixture["repository"], output, copy_function=shutil.copy2)
    binary = fixture.get("binary")
    if binary:
        # Copying changes inodes. Refresh the copy before applying measured edits.
        initialize_projection(output, binary, full_audit=False)
    hashes = {relative: hash_file(output / relative) for relative in fixture["input_sha256"]}
    files = sorted(key for key in hashes if key.startswith("knowledge/"))
    count = {"refresh_one": 1, "refresh_several": 2, "refresh_bulk": len(files)}.get(name, 0)
    changed = files[:count]
    for relative in changed:
        path = output / relative
        path.write_bytes(path.read_bytes().replace(b"Synthetic case", b"Altered___case"))
    command, request = build_request(name)
    expected = {"command": command, "id": FIRST_ID,
                "counts": fixture["counts"], "changed": bool(changed)}
    if command == "traverse":
        expected.update(build_traversal_oracle())
    return {"repository": output, "argv": [command], "stdin": encode_json(request),
            "expected": expected,
            "setup_seconds": time.monotonic() - started, "state_hashes": hashes,
            "cache_label": "initialized_projection_os_cache_uncontrolled",
            "embedding_mode": "fts_disabled", "changed_paths": changed, "name": name}


def build_traversal_oracle() -> dict:
    """Freeze the depth-two first-node neighborhood without product traversal."""
    def address(index):
        return f"knowledge/part-0000.md#node-{index:06d}"
    edges = []
    for index in range(4):
        edges.append([address(index), f"sources/shard-0000.txt#L{index + 1}-L{index + 1}", "external"])
        for offset in range(1, 5):
            kind = "includes" if offset == 4 and index % 4 < 3 else "depends_on"
            target = index + 1 if kind == "includes" else index + offset
            edges.append([address(index), address(target), kind])
    ids = [address(i) for i in range(1, 8)]
    ids.extend(f"sources/shard-0000.txt#L{i}-L{i}" for i in range(1, 5))
    return {"edges": edges, "element_ids": ids}


def build_request(name: str) -> tuple[str, dict]:
    """Return fixed requests; search edits use a unique FTS phrase."""
    if name == "inspect":
        return name, {"element_ids": [FIRST_ID]}
    if name == "traverse":
        return name, {"element_ids": [FIRST_ID], "mode": "both", "direction": "outgoing",
                      "depth": 2, "limit": 20, "edge_limit": 40}
    if name in ("check", "status", "reindex"):
        return name, {}
    return "search", {"query": '"case 000000"' if not name.startswith("refresh")
                       else '"Altered___case 000000"', "limit": 10}


def initialize_projection(repository: Path, binary: str, full_audit: bool = True) -> None:
    """Publish a validated projection outside the latency interval."""
    command = "check" if full_audit else "search"
    request = b"{}\n" if full_audit else b'{"query":"","limit":1}\n'
    output = subprocess.run([binary, command], input=request, cwd=repository,
                            env=get_environment(), capture_output=True, timeout=120, check=True)
    response = json.loads(output.stdout)
    expected = {"errors": []} if full_audit else []
    if response.get("ok") is not True or response.get("result") != expected:
        raise ValueError("fixture projection is invalid")


def verify_build(pin: dict) -> None:
    """Reject executable/source drift before a measured invocation."""
    required = {"executable", "sha256", "inputs", "product_commit", "rustc", "command", "release_debug", "target"}
    if not required <= pin.keys() or not {"--release", "--locked", "--offline"} <= set(pin["command"]):
        raise ValueError("missing locked release build provenance")
    binary = Path(pin["executable"])
    if binary.is_symlink() or hash_file(binary) != pin["sha256"]:
        raise ValueError("binary differs from pinned build")
    if read_product_inputs(ROOT) != pin["inputs"]:
        raise ValueError("product source differs from pinned build")


def read_product_inputs(source: Path) -> dict:
    """Hash actual product files and reject dirty or untracked build inputs."""
    files = run_git(source, "ls-files", "-z").decode().split("\0")
    names = [name for name in files if name in PRODUCT_FILES or name.startswith(PRODUCT_PREFIXES)]
    dirty = run_git(source, "status", "--porcelain", "--untracked-files=all", "--",
                    "src", "vendor", "vendored", "native", *sorted(PRODUCT_FILES))
    if dirty.strip():
        raise ValueError("product build inputs are dirty")
    return {name: hash_file(source / name) for name in names}


def verify_new_output(output: Path, source: Path | None = None) -> None:
    """Reject output reuse, symlinks and overlapping input/output roots."""
    if not output.is_absolute() or ".." in output.parts:
        raise ValueError("absolute normalized output required")
    if any(path.is_symlink() for path in (output, *output.parents)):
        raise ValueError("symlink output")
    if output.exists():
        raise ValueError("output already exists")
    if source and (output.is_relative_to(source) or source.is_relative_to(output)):
        raise ValueError("input/output overlap")


def get_environment() -> dict:
    """Use explicit Git configuration and discard ambient provider credentials."""
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("GIT_", "LETOPIS_", "OPENAI_", "OLLAMA_"))}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_TERMINAL_PROMPT="0", LC_ALL="C", TZ="UTC")
    return environment


def run_git(repository: Path, *arguments: str) -> bytes:
    """Execute fixed Git argv without inheriting user Git configuration."""
    return subprocess.check_output(["git", *arguments], cwd=repository,
                                   env=get_environment(), stderr=subprocess.PIPE, timeout=120)


def hash_file(path: Path) -> str:
    """Hash bytes without loading a corpus or executable into memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def encode_json(value: dict) -> bytes:
    return (json.dumps(value, sort_keys=True) + "\n").encode()


def write_json(path: Path, value: dict) -> None:
    """Exclusively create an evidence file; refuse earlier evidence replacement."""
    with path.open("xb") as stream:
        stream.write(encode_json(value))
