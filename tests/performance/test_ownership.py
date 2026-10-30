"""Fixture process identity is a private executable path, never a process name."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import os
import subprocess
import sys

from contention import configure_provider, embed_lease, invoke_cli, run_canary
from provider import start_provider

from ownership import FixtureProcesses


class OwnershipTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("PERF_LETOPIS_BINARY"), "requires pinned release executable")
    def test_stops_detached_successor_from_private_executable_only(self):
        # Given: a pending native drain uses a unique copy; unrelated work is alive.
        binary = Path(os.environ["PERF_LETOPIS_BINARY"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run_canary(binary, root / "canary")
            repository = root / "canary/repository"
            sentinel = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(60)"])
            try:
                with start_provider(root / "provider") as provider, FixtureProcesses(binary, root / "owned") as owner:
                    configure_provider(repository, provider.url, True)
                    with embed_lease(repository):
                        invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "", "limit": 1}})
                    provider.arm_barrier()
                    child = subprocess.Popen([str(owner.get_executable()), "embed", "--drain"], cwd=repository,
                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
                    try:
                        self.assertTrue(provider.arrived.wait(timeout=3))
                        # When: cleanup accounts for a setsid successor after its launcher is gone.
                        stopped = owner.stop_all()
                        child.wait(timeout=2)
                        # Then: exact private identity is stopped and unrelated work survives.
                        self.assertIn(child.pid, stopped)
                        self.assertIsNone(sentinel.poll())
                        self.assertEqual(owner.list_processes(), [])
                    finally:
                        if child.poll() is None:
                            child.kill()
                        child.wait()
            finally:
                sentinel.terminate()
                sentinel.wait()

    def test_identifies_only_the_unique_fixture_executable(self):
        # Given: unrelated processes include the same name or the path as an argument.
        owner = FixtureProcesses(Path("/unused/source"), Path("/private/tmp/unique-fixture"))
        program = str(owner.get_executable())
        rows = [(101, program + " embed --drain"), (102, "/usr/bin/letopis embed --drain"),
                (103, "echo " + program), (104, program + "-other embed --drain")]
        # When: identifying processes belonging to this fixture.
        with patch("ownership.read_process_rows", return_value=rows):
            found = owner.list_processes()
        # Then: no basename/substring matching can select another program.
        self.assertEqual(found, [101])

    def test_rechecks_identity_before_signalling_a_reused_pid(self):
        # Given: a previously observed PID now belongs to an unrelated executable.
        owner = FixtureProcesses(Path("/unused/source"), Path("/private/tmp/unique-fixture"))
        # When: the process identity changed between listing and signalling.
        with patch("ownership.read_process_command", return_value="/unrelated/program"), \
                patch("ownership.os.kill") as kill, patch("ownership.os.killpg") as killpg:
            owner.signal_process(101, 15)
        # Then: neither the new process nor its group is touched.
        kill.assert_not_called()
        killpg.assert_not_called()


if __name__ == "__main__":
    unittest.main()
