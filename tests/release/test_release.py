"""Behavioral checks for the release archive, independent of Cargo builds."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('release', REPO / 'scripts/release.py')
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class PackageTests(unittest.TestCase):
    def test_package_contains_only_declared_files_and_a_verifiable_receipt(self):
        # Given: a committed source tree, matching executable and unrelated file.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory))
            (root / 'unrelated.secret').write_text('must not be shipped')
            # When: an archive is built for a supported target.
            with patch.object(release, 'ROOT', root):
                archive = release.build_package(binary, output, 'aarch64-apple-darwin')
            # Then: only the executable, license and accurate receipt are shipped.
            self.assertEqual(archive.name, 'letopis-0.1.0-aarch64-apple-darwin.tar.gz')
            with tarfile.open(archive) as package:
                self.assertEqual(package.getnames(), ['letopis', 'LICENSE', 'BUILD.json'])
                self.assertEqual(package.getmember('letopis').mode, 0o755)
                self.assertEqual(package.extractfile('LICENSE').read(), b'MIT fixture\n')
                receipt = json.load(package.extractfile('BUILD.json'))
            self.assertEqual(receipt['version'], '0.1.0')
            self.assertEqual(receipt['target'], 'aarch64-apple-darwin')
            commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
            self.assertEqual(receipt['commit'], commit)
            expected = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(archive.with_suffix('.gz.sha256').read_text(), f'{expected}  {archive.name}\n')

    def test_package_rejects_unknown_target(self):
        # Given: valid source and an executable, but an unsupported target.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory))
            # When / Then: packaging fails instead of inventing a platform label.
            with patch.object(release, 'ROOT', root):
                with self.assertRaisesRegex(ValueError, 'unsupported target'):
                    release.build_package(binary, output, 'unknown-target')

    def test_package_rejects_a_different_binary_version(self):
        # Given: a 0.1.0 source tree paired with an older executable.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory), version='0.0.0')
            # When / Then: no archive is published with the wrong version inside.
            with patch.object(release, 'ROOT', root):
                with self.assertRaisesRegex(ValueError, 'binary version'):
                    release.build_package(binary, output, 'x86_64-unknown-linux-gnu')
            self.assertEqual(list(output.glob('*.tar.gz')), [])

    def test_package_does_not_overwrite_an_existing_archive(self):
        # Given: a previous archive whose bytes must remain intact.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory))
            output.mkdir()
            existing = output / 'letopis-0.1.0-x86_64-apple-darwin.tar.gz'
            existing.write_bytes(b'previous release')
            # When / Then: a second packaging attempt fails and preserves it.
            with patch.object(release, 'ROOT', root):
                with self.assertRaises(FileExistsError):
                    release.build_package(binary, output, 'x86_64-apple-darwin')
            self.assertEqual(existing.read_bytes(), b'previous release')

    def test_package_requires_a_license(self):
        # Given: a source tree with no license to distribute.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory))
            (root / 'LICENSE').unlink()
            # When / Then: packaging refuses an incomplete release.
            with patch.object(release, 'ROOT', root):
                with self.assertRaises(FileNotFoundError):
                    release.build_package(binary, output, 'aarch64-unknown-linux-gnu')


def create_fixture(directory, version='0.1.0'):
    root = directory / 'source'
    root.mkdir()
    (root / 'Cargo.toml').write_text('[package]\nversion = "0.1.0"\n')
    (root / 'LICENSE').write_bytes(b'MIT fixture\n')
    subprocess.run(['git', 'init', '-q', str(root)], check=True)
    subprocess.run(['git', 'add', '.'], cwd=root, check=True)
    subprocess.run(['git', '-c', 'user.name=Release Test', '-c', 'user.email=test@example.invalid',
                    '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture'], cwd=root, check=True)
    binary = directory / 'letopis'
    binary.write_text(f'#!/bin/sh\nprintf "letopis {version}\\n"\n')
    os.chmod(binary, 0o755)
    return root, binary, directory / 'output'


if __name__ == '__main__':
    unittest.main()
