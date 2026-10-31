"""Independently reread and check the generated fixture's bytes and graph."""

import argparse
from collections import deque
from dataclasses import dataclass, field
import hashlib
import json
import os
import posixpath
from pathlib import Path, PurePosixPath
import re
import subprocess

HEADER = re.compile(rb'^## Synthetic knowledge (\d{6,}) \{#(node-\d{6,})\}\n'
                    rb'<!-- letopis: (\{[^\n]+\}) -->\n', re.MULTILINE)
LINK = re.compile(r'\[([^\]\n]+)\]\(([^\s)]+) "letopis:type=([a-z][a-z0-9_]*);path=([^"\n]+)"\)')
SOURCE = re.compile(r'(sources/shard-\d{4,}\.txt)#L([1-9]\d*)-L([1-9]\d*)')


@dataclass
class Inspection:
    """Bounded per-file measurements and graph identities, never corpus payloads."""

    identities: set[str] = field(default_factory=set)
    edges: set[tuple[str, str, str]] = field(default_factory=set)
    fragments: list[int] = field(default_factory=list)
    files: list[int] = field(default_factory=list)
    distribution: list[int] = field(default_factory=list)


@dataclass
class _Fragment:
    identity: str
    metadata: dict
    data: bytes


def main() -> None:
    """Check a generated corpus manifest without invoking the product."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    arguments = parser.parse_args()
    try:
        manifest = validate_corpus(arguments.manifest)
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(1, f"validation failed: {error}\n")
    print(json.dumps(manifest["counts"], sort_keys=True))


def validate_corpus(manifest_path: Path) -> dict:
    """Verify fixture inputs independently and return the manifest on success.

    Resolve repository next to the manifest, never through an absolute manifest
    path. Raise ValueError on mismatched hashes, layout, graph, sizes or snapshots.
    Product-owned .letopis state and .git metadata are outside the input hash map.
    """
    _reject_symlinks(manifest_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    _validate_manifest(manifest)
    repository = manifest_path.parent / "repository"
    paths = _check_inputs(repository, manifest["input_sha256"])
    sources = _check_source_tree(repository, manifest["source_tree"], paths)
    inspection = _read_knowledge(repository, manifest["source_tree"], paths)
    source_count, includes = _check_graph(inspection, sources)
    _check_measurements(manifest, inspection, source_count, includes)
    return manifest


def _reject_symlinks(path: Path) -> None:
    if ".." in path.parts:
        raise ValueError("input contains parent traversal")
    if path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
        raise ValueError(f"symlink input: {path.name}")


def _validate_manifest(manifest: dict) -> None:
    if manifest.get("schema_version") != 1 or manifest.get("repository") != "repository":
        raise ValueError("unsupported corpus manifest")
    if not re.fullmatch(r"[0-9a-f]{40}", manifest.get("source_tree", "")):
        raise ValueError("invalid source tree identity")
    options = manifest["options"]
    expected = {"seed", "knowledge_count", "fragment_bytes", "elements_per_file"}
    if set(options) != expected or any(type(value) is not int for value in options.values()):
        raise ValueError("invalid corpus options")
    if (options["knowledge_count"] < 10 or options["fragment_bytes"] < 1000
            or not 10 <= options["elements_per_file"] <= 50):
        raise ValueError("invalid corpus options")


def _check_inputs(repository: Path, expected: dict[str, str]) -> list[str]:
    actual = set()
    for directory in ("knowledge", "sources"):
        root = repository / directory
        _reject_symlinks(root)
        for path in root.rglob("*"):
            _reject_symlinks(path)
            if path.is_file():
                actual.add(path.relative_to(repository).as_posix())
    if not actual or actual != set(expected):
        raise ValueError("input file inventory differs from manifest")
    for relative in sorted(actual):
        path = PurePosixPath(relative)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError("invalid input path")
        digest = hashlib.sha256((repository / relative).read_bytes()).hexdigest()
        if digest != expected[relative]:
            raise ValueError(f"input hash mismatch: {relative}")
    return sorted(actual)


def _check_source_tree(repository: Path, tree: str, paths: list[str]) -> dict[str, int]:
    listing = _read_git(repository, "ls-tree", "-r", tree).splitlines()
    objects = {}
    for entry in listing:
        description, relative = entry.split("\t", 1)
        mode, kind, digest = description.split()
        if mode != "100644" or kind != "blob" or not relative.startswith("sources/"):
            raise ValueError("source tree contains non-source entries")
        objects[relative] = digest
    source_paths = {path for path in paths if path.startswith("sources/")}
    if set(objects) != source_paths:
        raise ValueError("source tree file inventory mismatch")
    counts = {}
    for relative in sorted(source_paths):
        data = (repository / relative).read_bytes()
        digest = hashlib.sha1(f"blob {len(data)}\0".encode("ascii") + data).hexdigest()
        if digest != objects[relative]:
            raise ValueError("source tree content mismatch")
        data.decode("utf-8")
        lines = data.splitlines(keepends=True)
        if not lines or not data.endswith(b"\n") or b"\r" in data:
            raise ValueError("invalid source line contents")
        counts[relative] = len(lines)
    receipts = _read_git(repository, "cat-file", "--batch-check", stdin="\n".join(objects.values()) + "\n")
    if any(len(row.split()) != 3 or row.split()[1] != "blob" for row in receipts.splitlines()):
        raise ValueError("source tree blob unavailable")
    return counts


def _read_git(repository: Path, *arguments: str, stdin: str | None = None) -> str:
    environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    environment.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "LC_ALL": "C"})
    result = subprocess.run(["git", *arguments], cwd=repository, env=environment, input=stdin,
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise ValueError("source tree Git inspection failed")
    return result.stdout


def _read_knowledge(repository: Path, tree: str, paths: list[str]) -> Inspection:
    inspection = Inspection()
    for relative in paths:
        if not relative.startswith("knowledge/"):
            continue
        data = (repository / relative).read_bytes()
        data.decode("utf-8")
        headers = list(HEADER.finditer(data))
        if not headers or headers[0].start() != 0 or b"\r" in data:
            raise ValueError("invalid marked knowledge input")
        inspection.files.append(len(data))
        inspection.distribution.append(len(headers))
        for position, header in enumerate(headers):
            end = headers[position + 1].start() if position + 1 < len(headers) else len(data)
            fragment = _Fragment(f"{relative}#{header[2].decode('ascii')}",
                                 json.loads(header[3]), data[header.start():end])
            _read_fragment(inspection, fragment, tree)
    return inspection


def _read_fragment(inspection: Inspection, fragment: _Fragment, tree: str) -> None:
    metadata = fragment.metadata
    if (set(metadata) != {"created_at", "source_snapshot"}
            or type(metadata["created_at"]) is not int or metadata["created_at"] < 0
            or metadata["source_snapshot"] != tree):
        raise ValueError("invalid fragment metadata or source snapshot")
    identity = fragment.identity
    if identity in inspection.identities:
        raise ValueError("duplicate knowledge identity")
    inspection.identities.add(identity)
    inspection.fragments.append(len(fragment.data))
    text = fragment.data.decode("utf-8")
    if len(re.findall(r"^#{1,6} ", text, flags=re.MULTILINE)) != 1:
        raise ValueError("unexpected heading changes marked fragment boundary")
    if text.count("<!-- letopis:") != 1:
        raise ValueError("extra fragment metadata")
    if "`" in text or "\t" in text or re.search(r"^(?: {4}| {0,3}~{3})", text, re.MULTILINE):
        raise ValueError("code constructs are outside the generated fixture grammar")
    links = LINK.findall(text)
    if text.count('"letopis:type=') != len(links):
        raise ValueError("invalid graph link")
    for _, navigation, relation, target in links:
        containing_file = identity.split("#", 1)[0]
        navigation_file = posixpath.normpath(posixpath.join(
            posixpath.dirname(containing_file), navigation.split("#", 1)[0]))
        if navigation_file != target.split("#", 1)[0]:
            raise ValueError("navigation path disagrees with canonical target")
        edge = (identity, target, relation)
        if edge in inspection.edges:
            raise ValueError("duplicate edge triple")
        inspection.edges.add(edge)


def _check_graph(inspection: Inspection, source_lines: dict[str, int]) -> tuple[int, int]:
    sources = set()
    adjacency = {identity: [] for identity in inspection.identities}
    indegrees = dict.fromkeys(inspection.identities, 0)
    outgoing = dict.fromkeys(inspection.identities, 0)
    external = dict.fromkeys(inspection.identities, 0)
    for owner, target, relation in inspection.edges:
        outgoing[owner] += 1
        if relation == "external":
            _check_source(target, source_lines)
            sources.add(target)
            external[owner] += 1
            continue
        if target == owner:
            raise ValueError("self link")
        if target not in inspection.identities:
            raise ValueError("dangling internal target")
        if relation == "includes":
            adjacency[owner].append(target)
            indegrees[target] += 1
    _check_dag(adjacency, indegrees)
    if any(count != 5 for count in outgoing.values()):
        raise ValueError("each knowledge must own five edges")
    if any(count != 1 for count in external.values()) or len(sources) != len(external):
        raise ValueError("each knowledge must own one unique source")
    return len(sources), sum(len(targets) for targets in adjacency.values())


def _check_source(target: str, source_lines: dict[str, int]) -> None:
    match = SOURCE.fullmatch(target)
    if match is None:
        raise ValueError("invalid source address")
    path, first, last = match.groups()
    if path not in source_lines or not 1 <= int(first) <= int(last) <= source_lines[path]:
        raise ValueError("source range unavailable")


def _check_dag(adjacency: dict[str, list[str]], indegrees: dict[str, int]) -> None:
    queue = deque(identity for identity, degree in indegrees.items() if degree == 0)
    visited = 0
    while queue:
        owner = queue.popleft()
        visited += 1
        for target in adjacency[owner]:
            indegrees[target] -= 1
            if indegrees[target] == 0:
                queue.append(target)
    if visited != len(indegrees):
        raise ValueError("includes cycle")


def _check_measurements(manifest: dict, inspection: Inspection, sources: int, includes: int) -> None:
    options = manifest["options"]
    knowledge = len(inspection.identities)
    counts = {"knowledge": knowledge, "source": sources,
              "edges": len(inspection.edges), "includes": includes}
    if counts != manifest["counts"] or knowledge != options["knowledge_count"]:
        raise ValueError("counts mismatch")
    distribution = inspection.distribution
    packing = {"file_count": len(distribution), "elements_per_file": options["elements_per_file"],
               "last_file_elements": distribution[-1], "distribution": distribution}
    if packing != manifest["packing"] or any(not 10 <= size <= options["elements_per_file"]
                                               for size in distribution):
        raise ValueError("file packing mismatch")
    if (any(size != options["fragment_bytes"] for size in inspection.fragments)
            or _measure(inspection.fragments) != manifest["fragment_bytes"]):
        raise ValueError("fragment byte statistics mismatch")
    if _measure(inspection.files) != manifest["file_bytes"]:
        raise ValueError("file byte statistics mismatch")


def _measure(values: list[int]) -> dict:
    total = sum(values)
    return {"count": len(values), "sum": total, "mean": total / len(values),
            "min": min(values), "max": max(values)}


if __name__ == "__main__":
    main()
