"""Build deterministic, isolated Markdown/Git fixtures without product writes."""

import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import random
import subprocess

CREATED_AT = 1792792800
SOURCE_LINES = 100
MIN_ELEMENTS = 10
MAX_ELEMENTS = 50


@dataclass(frozen=True)
class CorpusOptions:
    """Explicit deterministic inputs; sizes count complete marked UTF-8 fragments."""

    seed: int
    knowledge_count: int = 100000
    fragment_bytes: int = 2000
    elements_per_file: int = 30


@dataclass
class _FragmentInputs:
    source_tree: str
    file_indices: list[int]
    options: CorpusOptions
    randomizer: random.Random


def main() -> None:
    """Generate a fixture with explicit CLI inputs; print its manifest location."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--nodes", type=int, default=100000)
    parser.add_argument("--seed", type=int, default=15)
    parser.add_argument("--fragment-bytes", type=int, default=2000)
    parser.add_argument("--elements-per-file", type=int, default=30)
    arguments = parser.parse_args()
    options = CorpusOptions(arguments.seed, arguments.nodes, arguments.fragment_bytes,
                            arguments.elements_per_file)
    try:
        generate_corpus(arguments.output, options)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"generation failed: {error}\n")
    print(arguments.output / "corpus.json")


def generate_corpus(output: Path, options: CorpusOptions) -> dict:
    """Create a new fixture and return its manifest; reject collisions and symlinks.

    Invalid parameters raise ValueError before creating output. I/O and Git failures
    propagate, retaining the partial directory for diagnosis rather than deleting it.
    """
    packing = _validate_options(options)
    _validate_output(output)
    output.mkdir(parents=True)
    repository = output / "repository"
    repository.mkdir()
    _run_git(repository, "init", "--quiet", "--template=", "--initial-branch=main",
             "--object-format=sha1")
    hashes = _write_sources(repository, options.knowledge_count)
    _run_git(repository, "add", "--", "sources")
    _run_git(repository, "commit", "--quiet", "-m", "Deterministic source snapshot")
    source_tree = _run_git(repository, "rev-parse", "HEAD^{tree}").strip()
    fragments, files, includes = _write_knowledge(repository, options, packing, source_tree)
    hashes.update({name: hashlib.sha256((repository / name).read_bytes()).hexdigest()
                   for name in files})
    manifest = {
        "schema_version": 1, "repository": "repository", "options": asdict(options),
        "source_tree": source_tree, "input_sha256": dict(sorted(hashes.items())),
        "counts": {"knowledge": options.knowledge_count, "source": options.knowledge_count,
                   "edges": options.knowledge_count * 5, "includes": includes},
        "packing": {"file_count": len(packing), "elements_per_file": options.elements_per_file,
                    "last_file_elements": packing[-1], "distribution": packing},
        "fragment_bytes": _get_stats(fragments), "file_bytes": _get_stats(list(files.values())),
    }
    (output / "corpus.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                                        encoding="utf-8", newline="\n")
    return manifest


def _validate_options(options: CorpusOptions) -> list[int]:
    packing = plan_packing(options.knowledge_count, options.elements_per_file)
    if type(options.seed) is not int:
        raise ValueError("seed must be an integer")
    if type(options.fragment_bytes) is not int or options.fragment_bytes < 1000:
        raise ValueError("fragment_bytes must be an integer of at least 1000")
    return packing


def plan_packing(knowledge_count: int, elements_per_file: int) -> list[int]:
    """Plan 10–50 elements per file, redistributing a short final remainder.

    Raise ValueError when the selected maximum cannot pack the requested count.
    This only allocates one integer per file and never renders payloads.
    """
    if type(knowledge_count) is not int or knowledge_count < MIN_ELEMENTS:
        raise ValueError("knowledge_count must be an integer of at least 10")
    if type(elements_per_file) is not int or not MIN_ELEMENTS <= elements_per_file <= MAX_ELEMENTS:
        raise ValueError("elements_per_file must be an integer between 10 and 50")
    full, remainder = divmod(knowledge_count, elements_per_file)
    packing = [elements_per_file] * full
    if remainder:
        packing.append(remainder)
    if packing[-1] >= MIN_ELEMENTS:
        return packing
    needed = MIN_ELEMENTS - packing[-1]
    if len(packing) < 2 or packing[-2] - needed < MIN_ELEMENTS:
        raise ValueError("impossible packing for selected elements_per_file")
    packing[-2] -= needed
    packing[-1] += needed
    return packing


def _validate_output(output: Path) -> None:
    if output.exists() or output.is_symlink():
        raise ValueError("output already exists")
    if any(parent.is_symlink() for parent in output.parents):
        raise ValueError("output has a symlink parent")


def _run_git(repository: Path, *arguments: str) -> str:
    environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    environment.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                        "GIT_AUTHOR_NAME": "Corpus Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                        "GIT_COMMITTER_NAME": "Corpus Fixture",
                        "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
                        "GIT_AUTHOR_DATE": f"{CREATED_AT} +0000",
                        "GIT_COMMITTER_DATE": f"{CREATED_AT} +0000", "LC_ALL": "C", "TZ": "UTC"})
    command = ["git", "-c", "core.autocrlf=false", "-c", "core.hooksPath=/dev/null",
               "-c", "commit.gpgSign=false", "-c", "core.excludesFile=/dev/null",
               "-c", "core.attributesFile=/dev/null", *arguments]
    return subprocess.run(command, cwd=repository, env=environment, check=True,
                          capture_output=True, text=True).stdout


def _write_sources(repository: Path, count: int) -> dict[str, str]:
    directory = repository / "sources"
    directory.mkdir()
    hashes = {}
    for start in range(0, count, SOURCE_LINES):
        relative = f"sources/shard-{start // SOURCE_LINES:04d}.txt"
        data = "".join(f"Source record {index:06d}: stable evidence for knowledge {index:06d}.\n"
                       for index in range(start, min(start + SOURCE_LINES, count))).encode("utf-8")
        (repository / relative).write_bytes(data)
        hashes[relative] = hashlib.sha256(data).hexdigest()
    return hashes


def _write_knowledge(repository: Path, options: CorpusOptions, packing: list[int],
                     source_tree: str) -> tuple[list[int], dict[str, int], int]:
    (repository / "knowledge").mkdir()
    file_indices = [file_index for file_index, size in enumerate(packing)
                    for _ in range(size)]
    inputs = _FragmentInputs(source_tree, file_indices, options, random.Random(options.seed))
    fragments, files, includes = [], {}, 0
    index = 0
    for file_index, size in enumerate(packing):
        relative = f"knowledge/part-{file_index:04d}.md"
        with (repository / relative).open("wb") as stream:
            for local in range(size):
                has_include = local % 4 < 3 and local + 1 < size
                fragment = _render_fragment(index, has_include, inputs)
                stream.write(fragment)
                fragments.append(len(fragment))
                includes += int(has_include)
                index += 1
        files[relative] = (repository / relative).stat().st_size
    return fragments, files, includes


def _render_fragment(index: int, has_include: bool, inputs: _FragmentInputs) -> bytes:
    metadata = json.dumps({"created_at": CREATED_AT, "source_snapshot": inputs.source_tree},
                          separators=(",", ":"))
    lines = [f"## Synthetic knowledge {index:06d} {{#node-{index:06d}}}\n",
             f"<!-- letopis: {metadata} -->\n",
             f"[source](sources/shard-{index // SOURCE_LINES:04d}.txt"
             f"#L{index % SOURCE_LINES + 1}-L{index % SOURCE_LINES + 1} \"letopis:type=external\")\n"]
    for offset in range(1, 5):
        target = (index + offset) % len(inputs.file_indices)
        relation = "depends_on"
        if offset == 4 and has_include:
            target, relation = index + 1, "includes"
        file_index = inputs.file_indices[target]
        lines.append(f'[relation {offset}](knowledge/part-{file_index:04d}.md#node-{target:06d} '
                     f'"letopis:type={relation}")\n')
    subject = inputs.randomizer.choice(("storage", "retrieval", "consistency", "recovery"))
    lines.append(f"\nSynthetic case {index:06d}: {subject}, sample {inputs.randomizer.randrange(1000000):06d}. "
                 "Данные проверяют воспроизводимость и связи.\n")
    prefix = "".join(lines).encode("utf-8")
    remaining = inputs.options.fragment_bytes - len(prefix) - 2
    if remaining < 0:
        raise ValueError("fragment_bytes cannot fit metadata and links")
    phrase = b"Evidence remains stable while the dependent knowledge is revised. "
    return prefix + (phrase * (remaining // len(phrase) + 1))[:remaining] + b"\n\n"


def _get_stats(values: list[int]) -> dict:
    return {"count": len(values), "sum": sum(values), "mean": sum(values) / len(values),
            "min": min(values), "max": max(values)}


if __name__ == "__main__":
    main()
