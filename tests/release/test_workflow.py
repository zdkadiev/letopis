"""Check the release workflow's publication and trigger contracts."""
import re
from pathlib import Path
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / '.github/workflows/release.yml'


class ReleaseWorkflowTests(unittest.TestCase):
    def test_only_version_tag_push_starts_release_build(self):
        # Given: the workflow's event configuration.
        source = WORKFLOW.read_text()
        events = source.split('\non:\n', 1)[1].split('\npermissions:', 1)[0]
        # When: its configured push triggers are inspected.
        triggers = re.findall(r'^    (\w+):\s*\[(.*?)\]\s*$', events, re.MULTILINE)
        # Then: version tags are the only configured push trigger or event.
        self.assertEqual(triggers, [('tags', "'v*'")])
        self.assertEqual(events.strip(), "push:\n    tags: ['v*']")

    def test_release_build_preserves_targets_without_publishing(self):
        # Given: the release workflow's complete configuration.
        source = WORKFLOW.read_text()
        # When: its targets, version validation and side effects are inspected.
        targets = re.findall(r'^\s+target: ([\w-]+)$', source, re.MULTILINE)
        # Then: all four packages are built without creating a release.
        self.assertCountEqual(targets, [
            'aarch64-apple-darwin', 'x86_64-apple-darwin',
            'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
        ])
        version_step = source.split('      - name: Verify version tag\n', 1)[1].split('\n      - name:', 1)[0]
        self.assertIn("if: startsWith(github.ref, 'refs/tags/')", version_step)
        self.assertIn("os.environ['GITHUB_REF_NAME'] != 'v' + version", version_step)
        self.assertIn('tag does not match package version', version_step)
        self.assertIn('name: package-${{ matrix.target }}', source)
        self.assertNotIn('gh release create', source)
        self.assertNotRegex(source, r'(?m)^  publish:')


if __name__ == '__main__':
    unittest.main()
