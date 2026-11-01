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
            with patch.object(release, 'ROOT', root), patch.object(release, 'get_third_party_notices', return_value=b'Dependency notices\n'):
                archive = release.build_package(binary, output, 'aarch64-apple-darwin')
            # Then: only the executable, license and accurate receipt are shipped.
            self.assertEqual(archive.name, 'letopis-0.1.0-aarch64-apple-darwin.tar.gz')
            with tarfile.open(archive) as package:
                self.assertEqual(package.getnames(), ['letopis', 'LICENSE', 'BUILD.json', 'THIRD_PARTY_NOTICES.txt'])
                self.assertEqual(package.getmember('letopis').mode, 0o755)
                self.assertEqual(package.extractfile('LICENSE').read(), b'MIT fixture\n')
                self.assertEqual(package.extractfile('THIRD_PARTY_NOTICES.txt').read(), b'Dependency notices\n')
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
            with patch.object(release, 'ROOT', root), patch.object(release, 'get_third_party_notices', return_value=b'Dependency notices\n'):
                with self.assertRaisesRegex(ValueError, 'unsupported target'):
                    release.build_package(binary, output, 'unknown-target')

    def test_package_rejects_a_different_binary_version(self):
        # Given: a 0.1.0 source tree paired with an older executable.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory), version='0.0.0')
            # When / Then: no archive is published with the wrong version inside.
            with patch.object(release, 'ROOT', root), patch.object(release, 'get_third_party_notices', return_value=b'Dependency notices\n'):
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
            with patch.object(release, 'ROOT', root), patch.object(release, 'get_third_party_notices', return_value=b'Dependency notices\n'):
                with self.assertRaises(FileExistsError):
                    release.build_package(binary, output, 'x86_64-apple-darwin')
            self.assertEqual(existing.read_bytes(), b'previous release')

    def test_package_requires_a_license(self):
        # Given: a source tree with no license to distribute.
        with tempfile.TemporaryDirectory() as directory:
            root, binary, output = create_fixture(Path(directory))
            (root / 'LICENSE').unlink()
            # When / Then: packaging refuses an incomplete release.
            with patch.object(release, 'ROOT', root), patch.object(release, 'get_third_party_notices', return_value=b'Dependency notices\n'):
                with self.assertRaises(FileNotFoundError):
                    release.build_package(binary, output, 'aarch64-unknown-linux-gnu')


    def test_notices_include_nested_dependency_and_standard_library_licenses(self):
        # Given: a dependency with a nested notice and a Rust copyright file.
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, _, _ = create_fixture(base)
            dep = base / 'dependency'
            (dep / 'nested').mkdir(parents=True)
            (dep / 'Cargo.toml').touch()
            (dep / 'LICENSE').write_text('Root permission text')
            (dep / 'nested/NOTICE').write_text('Nested copyright notice')
            rust = base / 'rust'
            (rust / 'share/doc/rust').mkdir(parents=True)
            (rust / 'share/doc/rust/COPYRIGHT-library.html').write_text('Rust license text')
            (root / 'vendor/vec1').mkdir(parents=True)
            (root / 'vendor/vec1/vec1.c').write_text('/* Vec1 blessing */\nsource bytes')
            metadata = {'packages': [{'name': 'dependency', 'version': '1.0.0',
                        'manifest_path': str(dep / 'Cargo.toml'), 'license': 'MIT',
                        'authors': ['Example Author'], 'repository': 'https://example.invalid'}]}
            # When: notices are collected from the locked dependency metadata.
            with patch.object(release, 'ROOT', root), patch.object(
                    release.subprocess, 'check_output', side_effect=[json.dumps(metadata), str(rust)]):
                notices = release.get_third_party_notices()
            # Then: all original notices, including nested files, are preserved.
            for expected in [b'Root permission text', b'Nested copyright notice',
                             b'Rust license text', b'Example Author', b'Vec1 blessing']:
                self.assertIn(expected, notices)


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
