"""TK-30  oracle fixtures; model-free. Shared fixtures are private."""
from pathlib import Path
import shutil
import tempfile
import unittest


class TempDirTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.evidence = self.tmp / "evidence"
        self.evidence.mkdir()
