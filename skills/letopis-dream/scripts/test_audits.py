"""Exercise the audit scripts with real Git trees and Markdown documents."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
ORGANIZATION_AUDIT = SCRIPTS.parents[1] / 'letopis' / 'scripts' / 'audit_knowledge.py'


class AuditsTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.run_git('init', '-q')

    def test_organization_audit_flags_more_than_forty_direct_concept_files(self):
        # Given: a folder contains 40 marked files, a child concept, and an unmarked page.
        knowledge = b'<!-- letopis: {"created_at":1700000000} -->\n# Concept\n'
        for index in range(40):
            self.write(f'letopis/broad/topic-{index}.md', knowledge)
        self.write('letopis/broad/child/detail.md', knowledge)
        self.write('letopis/broad/navigation.md', b'# Navigation\n')

        # When: the organization audit checks the folder and its child.
        status, report = self.run_script(ORGANIZATION_AUDIT)

        # Then: exactly 40 direct concept files are within the guideline.
        self.assertEqual(status, 0, report)
        self.assertEqual(report['concept_files_by_folder'], {
            'letopis/broad': 40,
            'letopis/broad/child': 1,
        })

        # Given: a 41st direct concept uses the other supported Markdown extension.
        self.write('letopis/broad/extra.markdown', knowledge)

        # When: the organization audit runs again.
        status, report = self.run_script(ORGANIZATION_AUDIT)

        # Then: only the parent folder is flagged for a split review.
        self.assertEqual(status, 1, report)
        self.assertEqual(report['problems'], [{
            'kind': 'folder_too_broad',
            'path': 'letopis/broad',
            'files': 41,
            'limit': 40,
        }])

    def test_snapshot_verifies_exact_inclusive_lines_and_distinguishes_remote(self):
        # Given: the source has CRLF, an embedded CR, and no final newline.
        self.write('src.txt', b'prefix\r\npayload\rbytes\r\nlast')
        self.run_git('-c', 'core.autocrlf=false', 'add', 'src.txt')
        tree = self.run_git('write-tree')
        receipt = self.receipt(tree, {
            'src.txt#L2-L3': {
                'path': 'src.txt',
                'lines': [2, 3],
                'hash': hashlib.sha256(b'payload\rbytes\r\nlast').hexdigest(),
            },
            'HTTPS://example.test/a/../source': {
                'path': 'HTTPS://example.test/a/../source',
                'hash': None,
            },
        })
        self.write('src.txt', b'worktree bytes differ from the snapshot')

        # When: the snapshot receipt is verified from Git.
        status, report = self.run_script('verify_snapshot.py', request=receipt)

        # Then: the local address matches and the remote address is reported separately.
        self.assertEqual(status, 0, report)
        self.assertEqual(report['verified'], ['src.txt#L2-L3'])
        self.assertEqual(report['remote_sources'], ['HTTPS://example.test/a/../source'])
        self.assertEqual(report['findings'], [])

    def test_snapshot_reports_wrong_hash_and_missing_local_evidence(self):
        # Given: a receipt has an incorrect hash and an unavailable local address.
        self.write('src.txt', b'source\n')
        self.run_git('add', 'src.txt')
        tree = self.run_git('write-tree')
        receipt = self.receipt(tree, {
            'src.txt': {'path': 'src.txt', 'hash': '0' * 64},
            'src.txt#L2-L3': {'path': 'src.txt', 'lines': [2, 3], 'hash': None},
        })

        # When: the snapshot receipt is verified.
        status, report = self.run_script('verify_snapshot.py', request=receipt)

        # Then: both local failures prevent a successful result.
        self.assertEqual(status, 1, report)
        self.assertEqual(
            {(finding['element_id'], finding['reason']) for finding in report['findings']},
            {('src.txt', 'hash_mismatch'), ('src.txt#L2-L3', 'missing_local_hash')},
        )

    def test_snapshot_rejects_commit_oid(self):
        # Given: the receipt points to a commit, which Git could otherwise peel to a tree.
        self.write('src.txt', b'source\n')
        self.run_git('add', 'src.txt')
        self.run_git('-c', 'user.name=Fixture', '-c', 'user.email=test@example.test',
                     'commit', '-qm', 'Add source')
        receipt = self.receipt(self.run_git('rev-parse', 'HEAD'), {
            'src.txt': {'path': 'src.txt', 'hash': hashlib.sha256(b'source\n').hexdigest()},
        })

        # When: the receipt is checked.
        status, report = self.run_script('verify_snapshot.py', request=receipt)

        # Then: a commit OID cannot satisfy the snapshot contract.
        self.assertEqual(status, 2, report)
        self.assertIn('tree', report['error'])

    @unittest.skipUnless(shutil.which('pandoc'), 'Pandoc is needed for navigation tests')
    def test_navigation_checks_fragments_and_line_bounds(self):
        # Given: valid anchors, encoded filenames, reference links, and fenced examples.
        self.write('letopis/target file.markdown', (
            b'# Decision {#decision}\n\n## Detail\n\n## Detail\n\n<a id="manual"></a>\n'
        ))
        self.write('letopis/literal%20.md', b'# Percent\n')
        self.write('src.txt', b'first\r\nsecond')
        self.write('letopis/overview.md', (
            b'# Overview\n\n'
            b'[Heading](target%20file.markdown#detail-1) [Manual][ref]\n'
            b'[Percent](literal%2520.md#percent) [Range](../src.txt#L1-L2)\n'
            b'[Mistyped](target%20file.markdown#absent '
            b'"letopis:type=related_to;path=letopis/target file.markdown#decision")\n'
            b'[Different anchor](target%20file.markdown#manual '
            b'"letopis:type=related_to;path=letopis/target file.markdown#decision")\n'
            b'[Wrong case](Target%20file.markdown#decision)\n'
            b'[Missing range](../src.txt#L2-L3) [Missing file](retired.md)\n'
            b'[Remote](https://example.test/unknown#skip)\n\n'
            b'[ref]: target%20file.markdown#manual\n\n'
            b'```md\n[Example](ignore.md)\n```\n'
        ))

        # When: the skill audits the default knowledge folder.
        status, report = self.run_script('check_navigation.py')

        # Then: the broken navigation is reported even when the marked title is correct.
        self.assertEqual(status, 1, report)
        self.assertEqual(report['files_checked'], 3)
        self.assertEqual(
            {(item['destination'], item['reason']) for item in report['findings']},
            {
                ('target%20file.markdown#absent', 'missing_fragment'),
                ('target%20file.markdown#manual', 'fragment_mismatch'),
                ('Target%20file.markdown#decision', 'missing_target'),
                ('../src.txt#L2-L3', 'line_out_of_range'),
                ('retired.md', 'missing_target'),
            },
        )

    @unittest.skipUnless(shutil.which('pandoc'), 'Pandoc is needed for navigation tests')
    def test_navigation_backlinks_find_references_outside_knowledge_folder(self):
        # Given: an outside document refers to a deleted concept through a reference link.
        self.write('docs/runbook.md', (
            b'[Original concept][old]\n\n'
            b'[old]: ../letopis/old%20concept.md#facts\n'
            b'[Unrelated](elsewhere.md)\n'
        ))

        # When: the checker scans inbound links for the deleted file.
        status, report = self.run_script('check_navigation.py', '--target', 'letopis/old concept.md')

        # Then: the backlink survives in the report even though its file has been deleted.
        self.assertEqual(status, 1, report)
        self.assertEqual(len(report['backlinks']), 1)
        self.assertEqual(report['backlinks'][0]['path'], 'docs/runbook.md')
        self.assertEqual(report['backlinks'][0]['fragment'], 'facts')
        self.assertEqual([item['reason'] for item in report['findings']], ['missing_target'])

    def write(self, path, contents):
        destination = self.root / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)

    def run_git(self, *arguments):
        return subprocess.check_output(['git', *arguments], cwd=self.root, text=True).strip()

    def run_script(self, script, *arguments, request=None):
        completed = subprocess.run(
            [sys.executable, str(SCRIPTS / script), *arguments],
            cwd=self.root,
            input=json.dumps(request),
            text=True,
            capture_output=True,
        )
        self.assertTrue(completed.stdout, completed.stderr)
        return completed.returncode, json.loads(completed.stdout)

    @staticmethod
    def receipt(tree, elements):
        return {'ok': True, 'result': {'snapshot': tree, 'elements': elements}}


if __name__ == '__main__':
    unittest.main()
