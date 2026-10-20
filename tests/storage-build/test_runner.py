import hashlib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import tempfile
from threading import Event
import unittest
from unittest.mock import patch

from run import checked_source


class SourceVerification(unittest.TestCase):
    # A checksum mismatch must prevent a native source file reaching the compiler.
    def test_rejects_corrupt_download_before_publishing_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / "origin.c"
            origin.write_bytes(b"corrupt")
            destination = root / "vec1.c"
            with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                checked_source(destination, origin.as_uri(), hashlib.sha256(b"abc").hexdigest())
            self.assertFalse(destination.exists())

    def test_rejects_corrupt_cached_source_without_replacing_it(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "vec1.c"
            source.write_bytes(b"corrupt")
            with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                checked_source(source, "https://unused.invalid", hashlib.sha256(b"abc").hexdigest())
            self.assertEqual(source.read_bytes(), b"corrupt")

    def test_accepts_matching_download_and_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / "origin.c"
            origin.write_bytes(b"abc")
            destination = root / "vec1.c"
            digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            self.assertEqual(checked_source(destination, origin.as_uri(), digest), destination)
            self.assertEqual(checked_source(destination, "https://unused.invalid", digest), destination)
            self.assertEqual(destination.read_bytes(), b"abc")

    def test_partial_write_is_not_visible_to_cache_readers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / "origin.c"
            origin.write_bytes(b"abc")
            destination = root / "vec1.c"
            partial_written, finish_write = Event(), Event()
            write_bytes = Path.write_bytes

            def delayed_write(path, data):
                # Inject a pause in a real filesystem write, exposing a partial file.
                write_bytes(path, data[:1])
                partial_written.set()
                if not finish_write.wait(timeout=5):
                    raise TimeoutError("test did not release the source writer")
                return write_bytes(path, data)

            with patch.object(Path, "write_bytes", delayed_write), ThreadPoolExecutor(max_workers=1) as pool:
                future = pool.submit(checked_source, destination, origin.as_uri(),
                                     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
                try:
                    self.assertTrue(partial_written.wait(timeout=5))
                    self.assertFalse(destination.exists(), "a reader could consume partial source")
                finally:
                    finish_write.set()
                self.assertEqual(future.result(timeout=5), destination)
            self.assertEqual(destination.read_bytes(), b"abc")


if __name__ == "__main__":
    unittest.main()
