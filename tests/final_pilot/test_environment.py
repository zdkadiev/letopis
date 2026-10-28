"""Model-free tests for the TK-30 build context and local environment preflight.

Docker and Codex are stub executables; the only real Docker test is opt-in and uses an
already-local image with `--pull never`. Auth paths are FIFO traps: opening one blocks,
so any read makes the subprocess time out instead of passing.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

import environment
import study
from test_study import fill_every_input

HERE = Path(__file__).resolve().parent
RUN = HERE / "run.py"
BASE = "0123456789abcdef0123456789abcdef01234567"
PINNED_IMAGE = "sha256:" + "a" * 64
STALE_IMAGE = "sha256:" + "b" * 64
TOOLS_OK = "codex-cli 0.164.0\n"
MEMORY_OK = '{"knowledge_nodes": 0}\n'
ISOLATED = "isolated\n"
ALWAYS_PENDING = ["preflight:git_target_separation", "participant_isolation", "model_access",
                  "embedding_access"]
PREFLIGHT_IMAGE = os.environ.get("TK30_PREFLIGHT_IMAGE")
TRAP_TIMEOUT_SECONDS = 30

DOCKER_STUB = '''#!{python}
import json, os, sys
argv = sys.argv[1:]
with open({log!r}, "a") as out:
    out.write(json.dumps({{"argv": argv, "env": sorted(os.environ)}}) + "\\n")
responses = json.load(open({responses!r}))
if argv[:2] == ["image", "inspect"]:
    if responses["image_id"] is None:
        sys.exit(1)
    print(responses["image_id"])
    sys.exit(0)
name = argv[-1].split("\\n", 1)[0].removeprefix("# tk30-check:")
sys.stdout.write(responses[name])
sys.exit(0)
'''


class ContextTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.source = self.tmp / "source"
        (self.source / "src").mkdir(parents=True)
        (self.source / "src/lib.rs").write_text("pub fn a() {}\n")
        self.assets = self.tmp / "assets"
        self.assets.mkdir()

    def test_context_copies_source_and_records_verified_asset_hashes(self):
        # Given
        archive, member = "codex.tar.gz", "codex-bin"
        digest = write_asset(self.assets / archive, member, b"pinned codex bytes")

        # When
        with mock.patch.object(environment, "PINNED_ASSETS", ((archive, member, digest),)):
            result = environment.prepare_context(self.source, self.assets, self.tmp / "ctx")

        # Then
        context = self.tmp / "ctx"
        self.assertEqual(result["pending"], [])
        self.assertEqual(result["model_executions"], 0)
        self.assertEqual(result["asset_sha256"], {archive: digest})
        self.assertEqual(result["codex_version"], "0.164.0")
        self.assertEqual((context / "source/src/lib.rs").read_text(), "pub fn a() {}\n")
        self.assertEqual((context / member).read_bytes(), b"pinned codex bytes")
        saved = json.loads((context / "provenance.json").read_text())
        self.assertEqual(saved["source"]["items"], 1)
        self.assertIn("tests/early_pilot/prepare_image.py", saved["tk17_sources"]["files"])
        self.assertNotIn("auth", (context / "Dockerfile").read_text())

    def test_corrupted_asset_is_rejected_before_any_output(self):
        # Given: bytes that differ from the recorded TK-17 release digests
        for name, _, _ in environment.PINNED_ASSETS:
            (self.assets / name).write_bytes(b"corrupted")

        # When / Then
        with self.assertRaisesRegex(ValueError, "asset differs"):
            environment.prepare_context(self.source, self.assets, self.tmp / "ctx")
        self.assertFalse((self.tmp / "ctx").exists())

    def test_missing_assets_stay_pending_by_exact_name_without_download(self):
        # When
        result = environment.prepare_context(self.source, self.assets, self.tmp / "ctx")

        # Then
        self.assertEqual(result["pending"], ["asset:codex-linux.tar.gz",
                                             "asset:code-mode-host.tar.gz"])
        self.assertIsNone(result["context"])
        self.assertFalse((self.tmp / "ctx").exists())

    def test_source_with_git_or_credentials_is_rejected(self):
        # Given
        archive, member = "codex.tar.gz", "codex-bin"
        digest = write_asset(self.assets / archive, member, b"pinned codex bytes")
        for leaked in (".git/HEAD", ".codex/auth.json", "nested/auth.json", ".engram/db"):
            with self.subTest(leaked):
                source = self.tmp / leaked.replace("/", "_")
                shutil.copytree(self.source, source)
                (source / leaked).parent.mkdir(parents=True, exist_ok=True)
                (source / leaked).write_text("secret\n")

                # When / Then
                with mock.patch.object(environment, "PINNED_ASSETS",
                                       ((archive, member, digest),)):
                    with self.assertRaisesRegex(ValueError, "must not contain"):
                        environment.prepare_context(source, self.assets, self.tmp / "out")
                self.assertFalse((self.tmp / "out").exists())


class PreflightTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.root = self.tmp / "root"
        self.root.mkdir()
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        self.log = self.tmp / "docker.jsonl"
        self.responses = self.tmp / "responses.json"
        install_docker_stub(self.bin, self.log, self.responses)
        self.path = f"{self.bin}{os.pathsep}{os.environ['PATH']}"

    def write_manifest(self, manifest):
        (self.root / "manifest.json").write_text(json.dumps(manifest))

    def answer(self, image_id=PINNED_IMAGE, tools=TOOLS_OK, memory=MEMORY_OK,
               isolation=ISOLATED):
        self.responses.write_text(json.dumps({"image_id": image_id, "tools": tools,
                                              "memory": memory,
                                              "container_isolation": isolation}))

    def run_preflight(self):
        with mock.patch.dict(os.environ, {"PATH": self.path}):
            return environment.preflight(self.root, "letopis-final:pinned")

    def read_docker_calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_passing_local_checks_never_clear_model_or_embedding_access(self):
        # Given: every freeze input supplied and the pinned image matches
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer()

        # When
        result = self.run_preflight()

        # Then
        self.assertEqual({name: check["passed"] for name, check in result["checks"].items()},
                         {"image_digest": True, "tools": True, "memory": True,
                          "container_isolation": True})
        self.assertEqual(result["pending"], ALWAYS_PENDING)
        self.assertEqual(result["model_executions"], 0)

    def test_missing_product_model_or_gate_stays_unready(self):
        # Given: local tools pass, but model/product/G-2/config are null
        self.write_manifest(build_manifest(image=PINNED_IMAGE))
        self.answer()

        # When
        result = self.run_preflight()

        # Then
        self.assertTrue(all(check["passed"] for check in result["checks"].values()))
        for name in ("product", "embedding", "model", "agent_config", "g2"):
            self.assertIn(name, result["pending"])
        self.assertEqual(result["pending"][-4:], ALWAYS_PENDING)

    def test_local_smoke_never_clears_git_target_or_participant_isolation(self):
        # Given: every input supplied and every local probe passing
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer()

        # When
        result = self.run_preflight()

        # Then
        self.assertIn("preflight:git_target_separation", result["pending"])
        self.assertIn("participant_isolation", result["pending"])
        self.assertNotIn("isolation", result["checks"])
        self.assertTrue(result["checks"]["container_isolation"]["passed"])
        self.assertTrue(result["checks"]["container_isolation"]["detail"].startswith(
            "preflight container only; not the participant run"))

    def test_cli_version_mismatch_fails_the_tools_check(self):
        # Given
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer(tools="codex-cli 0.163.0\n")

        # When
        result = self.run_preflight()

        # Then
        self.assertFalse(result["checks"]["tools"]["passed"])
        self.assertIn("preflight:tools", result["pending"])

    def test_missing_tool_or_non_empty_memory_fails(self):
        # Given
        cases = {"tools": {"tools": "missing:rg\n" + TOOLS_OK},
                 "memory": {"memory": '{"knowledge_nodes": 2}\n'},
                 "container_isolation": {"isolation": "egress:open\n"}}
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        for name, answer in cases.items():
            with self.subTest(name):
                self.answer(**answer)

                # When
                result = self.run_preflight()

                # Then
                self.assertFalse(result["checks"][name]["passed"])
                self.assertIn(f"preflight:{name}", result["pending"])

    def test_stale_image_digest_is_not_executed(self):
        # Given
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer(image_id=STALE_IMAGE)

        # When
        result = self.run_preflight()

        # Then
        self.assertFalse(result["checks"]["image_digest"]["passed"])
        self.assertEqual(set(result["checks"]), {"image_digest"})
        self.assertIn("preflight:image_digest", result["pending"])
        self.assertEqual([call["argv"][:2] for call in self.read_docker_calls()],
                         [["image", "inspect"]])

    def test_absent_local_image_is_pending_without_pull(self):
        # Given
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer(image_id=None)

        # When
        result = self.run_preflight()

        # Then
        self.assertIsNone(result["checks"]["image_digest"]["passed"])
        self.assertIn("preflight:image_digest", result["pending"])
        self.assertNotIn("pull", [arg for call in self.read_docker_calls()
                                  for arg in call["argv"]])

    def test_containers_get_no_network_mounts_env_or_pull(self):
        # Given
        self.write_manifest(build_manifest(image=PINNED_IMAGE, filled=True))
        self.answer()

        # When
        self.run_preflight()

        # Then
        runs = [call["argv"] for call in self.read_docker_calls() if call["argv"][0] == "run"]
        self.assertEqual(len(runs), 3)
        for argv in runs:
            self.assertIn("--network none", " ".join(argv))
            self.assertIn("--pull never", " ".join(argv))
            self.assertEqual(argv[argv.index("bash") + 1], PINNED_IMAGE)
            for flag in ("-v", "--volume", "--mount", "-e", "--env", "--privileged"):
                self.assertNotIn(flag, argv)
            self.assertNotIn("codex exec", argv[-1])

    def test_memory_is_checked_without_provider_config(self):
        # Given: an embedding config value that must stay an operator input
        manifest = build_manifest(image=PINNED_IMAGE, filled=True)
        manifest["product"]["embedding_config"] = "provider=remote-embedding-unit"
        self.write_manifest(manifest)
        self.answer()

        # When
        result = self.run_preflight()

        # Then
        memory = next(call["argv"][-1] for call in self.read_docker_calls()
                      if call["argv"][-1].startswith("# tk30-check:memory"))
        self.assertNotIn("remote-embedding-unit", memory)
        self.assertNotIn("embed", memory)
        self.assertIn("embedding_access", result["pending"])


class CredentialTrapTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_prepare_never_reads_auth_or_executes_model(self):
        # Given: FIFO auth traps, a token in the environment and a host codex marker
        env, marker, log = self.build_trapped_environment()
        root = self.tmp / "root"
        root.mkdir()
        (root / "manifest.json").write_text(json.dumps(build_manifest(image=PINNED_IMAGE)))
        (self.tmp / "assets").mkdir()

        # When
        preflight = self.run_cli(env, "preflight", "--root", root, "--image", "local:tag")
        context = self.run_cli(env, "context", "--source", self.tmp / "assets",
                               "--assets", self.tmp / "assets", "--output", self.tmp / "ctx")

        # Then
        self.assertEqual(preflight[0], 2)
        self.assertEqual(preflight[1]["model_executions"], 0)
        self.assertIn("model_access", preflight[1]["pending"])
        self.assertEqual(context[0], 2)
        self.assertIn("asset:codex-linux.tar.gz", context[1]["pending"])
        self.assertFalse(marker.exists())
        calls = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertTrue(calls)
        for call in calls:
            self.assertNotIn("OPENAI_API_KEY", call["env"])
            self.assertNotIn("CODEX_HOME", call["env"])
            self.assertNotIn("codex exec", " ".join(call["argv"]))

    def build_trapped_environment(self):
        codex_home = self.tmp / "codex_home"
        (self.tmp / ".codex").mkdir()
        codex_home.mkdir()
        os.mkfifo(codex_home / "auth.json")
        os.mkfifo(self.tmp / ".codex/auth.json")
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        marker = self.tmp / "codex-started"
        shim = bin_dir / "codex"
        shim.write_text(f"#!/bin/sh\ntouch '{marker}'\n")
        shim.chmod(0o755)
        log, responses = self.tmp / "docker.jsonl", self.tmp / "responses.json"
        responses.write_text(json.dumps({"image_id": PINNED_IMAGE, "tools": TOOLS_OK,
                                         "memory": MEMORY_OK,
                                         "container_isolation": ISOLATED}))
        install_docker_stub(bin_dir, log, responses)
        env = dict(os.environ, CODEX_HOME=str(codex_home), HOME=str(self.tmp),
                   OPENAI_API_KEY="trap-token",
                   PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}")
        return env, marker, log

    def run_cli(self, env, *args):
        result = subprocess.run([sys.executable, "-B", str(RUN), *map(str, args)],
                                cwd=self.tmp, capture_output=True, text=True, env=env,
                                timeout=TRAP_TIMEOUT_SECONDS)
        return result.returncode, json.loads(result.stdout)


@unittest.skipUnless(PREFLIGHT_IMAGE, "set TK30_PREFLIGHT_IMAGE to an already-local image to "
                     "run the real Docker preflight; nothing is pulled or built")
class LocalImagePreflightTests(unittest.TestCase):
    def test_real_image_preflight_stays_unready_without_model_runs(self):
        # Given
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "manifest.json").write_text(json.dumps(build_manifest(image=None)))

            # When
            result = environment.preflight(root, PREFLIGHT_IMAGE)

        # Then
        print(json.dumps(result), file=sys.stderr)
        self.assertEqual(result["model_executions"], 0)
        self.assertEqual(result["pending"][-4:], ALWAYS_PENDING)
        self.assertIn("preflight:image_digest", result["pending"])
        self.assertEqual(set(result["checks"]), {"image_digest", "tools", "memory",
                                                 "container_isolation"})


def build_manifest(image, filled=False):
    selection = study.load_selection(HERE.parent.parent
                                     / "docs/zkit/letopis/pilot/final/selection.json")
    manifest = study.build_manifest(selection, {"original_base": BASE, "file_sha256": {}})
    if filled:
        manifest = fill_every_input(manifest)
    manifest["environment"]["image"] = image
    return manifest


def install_docker_stub(bin_dir, log, responses):
    stub = bin_dir / "docker"
    stub.write_text(DOCKER_STUB.format(python=sys.executable, log=str(log),
                                       responses=str(responses)))
    stub.chmod(0o755)


def write_asset(path, member, data):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        info = tarfile.TarInfo(member)
        info.size = len(data)
        archive.addfile(info, io.BytesIO(data))
    path.write_bytes(buffer.getvalue())
    return hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()
