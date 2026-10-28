"""TK-30 test copy containers; model-free. Shared fixtures are private."""
import json
import os
from pathlib import Path
import subprocess
import unittest
import uuid
import copies
from _copy_fixtures import (
    AGENTS_ORIGINAL,
    BASE_FILES,
    GIT_ENV,
    SLOT,
    SUPERPOWERS_BULLET,
    commit,
    git,
    make_source_repo,
    write_tree,
)


DOCKER_OUTPUT = os.environ.get("TK30_CONTAINER_OUTPUT")


DOCKER_IMAGE = os.environ.get("TK30_TEST_IMAGE", "letopis-tk17-codex:v2-0.164.0")


TOY_FILES = {
    "AGENTS.md": (AGENTS_ORIGINAL, 0o644),
    "Cargo.toml": ('[package]\nname="tk30_copy_probe"\nversion="0.0.0"\nedition="2024"\n', 0o644),
    "src/lib.rs": ("pub fn get_number() -> u8 { 7 }\n", 0o644),
    "build.rs": ('fn main() { let data = std::fs::read("/oracle/marker").unwrap();\n'
                 'let target = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")'
                 '.unwrap()).join("target"); std::fs::create_dir_all(&target).unwrap();\n'
                 'std::fs::write(target.join("leak-attempt"), data).unwrap(); }\n', 0o644),
}


MARKER = "TK30_PRIVATE_ORACLE_MARKER"


DOCKER_BASE = ["docker", "run", "--rm", "--pull", "never", "--network", "none",
               "--cap-drop", "ALL", "--security-opt", "no-new-privileges"]


class DockerArgumentTests(unittest.TestCase):
    def test_boundary_proof_never_pulls_an_image(self):
        # When
        argv = " ".join(DOCKER_BASE)

        # Then
        self.assertIn("--pull never", argv)
        self.assertIn("--network none", argv)


@unittest.skipUnless(DOCKER_OUTPUT, "set TK30_CONTAINER_OUTPUT to a new private evidence "
                                    "directory to run the model-free Docker boundary proof")
class ContainerBoundaryTests(unittest.TestCase):
    def test_build_script_writes_only_the_disposable_copy(self):
        # Given
        root = Path(DOCKER_OUTPUT).resolve()
        root.mkdir(mode=0o700, parents=True, exist_ok=False)
        (root / "source-parent").mkdir()
        source, base = make_source_repo(root / "source-parent", TOY_FILES)
        study = root / "study"
        copies.prepare_seeds(source, base, study)
        capture = copies.capture_slot(study, SLOT)
        evaluation = copies.prepare_evaluation(study, SLOT)
        oracle = root / "oracle"
        oracle.mkdir()
        (oracle / "marker").write_text(MARKER)
        run_id = uuid.uuid4().hex[:12]

        # When
        build = run_evaluator_build(run_id, evaluation, oracle, root / "build.log")
        denial = run_future_slot_probe(run_id, study, source, root / "t2-denial.log")
        unsafe = run_shared_mount_write(run_id, study, oracle, root / "unsafe.log")

        # Then
        self.assertEqual(build.returncode, 0, (root / "build.log").read_text())
        self.assertEqual((evaluation / "repo/target/leak-attempt").read_text(), MARKER)
        self.assertEqual(denial.returncode, 0, (root / "t2-denial.log").read_text())
        self.assertEqual(unsafe.returncode, 0)
        with self.assertRaises(ValueError):
            copies.require_unchanged(study, capture)
        self.assert_owned_timeout_leaves_no_container(root)
        (root / "receipt.json").write_text(json.dumps({
            "status": "PASS", "model_executions": 0, "image": DOCKER_IMAGE,
            "tk17_sources_commit": copies.SOURCE_RECEIPTS["commit"],
            "build_script_writeback_only_disposable": True,
            "future_t2_marker_denied": True, "unsafe_shared_mount_detected": True,
            "owned_timeout_removed": True}, indent=2) + "\n")

    def assert_owned_timeout_leaves_no_container(self, root):
        run_id = uuid.uuid4().hex[:12]
        listing = ["docker", "ps", "-a", "--filter", f"label=tk30-run={run_id}", "--format",
                   "{{.Names}}"]
        with self.assertRaises(subprocess.TimeoutExpired):
            run_owned_container(run_id, ["--entrypoint", "sh", DOCKER_IMAGE, "-c", "sleep 30"],
                                root / "timeout.log", 2)
        self.assertEqual(subprocess.check_output(listing, text=True), "")


def run_evaluator_build(run_id, evaluation, oracle, log):
    """Build the evaluation copy with the private oracle mounted and a fresh cargo home."""
    return run_owned_container(run_id, [
        "-v", f"{evaluation / 'repo'}:/workspace", "-v", f"{evaluation / 'target'}:/operator-target",
        "-v", f"{evaluation / 'home'}:/home/eval", "-v", f"{oracle}:/oracle:ro",
        "--workdir", "/workspace", "--env", "CARGO_HOME=/tmp/fresh-cargo",
        "--env", "CARGO_TARGET_DIR=/operator-target", "--entrypoint", "sh", DOCKER_IMAGE,
        "-c", "cargo build --offline --quiet"], log, 180)


def run_future_slot_probe(run_id, study, source, log):
    """Mount only what the next slot gets and assert no oracle, target or source is visible."""
    home = copies.create_home(study, "C1-T2-baseline")
    probe = ("test ! -e /oracle && test ! -e /operator-target "
             "&& test ! -e /workspace/target/leak-attempt "
             f"&& test ! -e {source} && ! grep -rq {MARKER} /workspace /home/pilot")
    return run_owned_container(run_id, [
        "-v", f"{study / 'conditions/C1-baseline/repo'}:/workspace", "-v", f"{home}:/home/pilot",
        "--workdir", "/workspace", "--entrypoint", "sh", DOCKER_IMAGE, "-c", probe], log, 60)


def run_shared_mount_write(run_id, study, oracle, log):
    """Unsafe control: the oracle is copied into the participant repository itself."""
    return run_owned_container(run_id, [
        "-v", f"{study / 'conditions/C1-baseline/repo'}:/workspace", "-v", f"{oracle}:/oracle:ro",
        "--entrypoint", "sh", DOCKER_IMAGE, "-c",
        "mkdir -p /workspace/target && cp /oracle/marker /workspace/target/leak-attempt"], log, 60)


def run_owned_container(run_id, arguments, log, timeout):
    """Run one labelled container; on timeout stop and remove only that exact name."""
    name = f"tk30-eval-{run_id}-{uuid.uuid4().hex[:6]}"
    command = [*DOCKER_BASE, "--name", name, "--label", f"tk30-run={run_id}", *arguments]
    with log.open("wb") as output:
        try:
            return subprocess.run(command, stdout=output, stderr=output, timeout=timeout)
        except subprocess.TimeoutExpired:
            subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=30)
            raise


if __name__ == "__main__":
    unittest.main()
