"""Archive inputs must not gain host/container paths through extraction."""
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("runner", Path(__file__).with_name("environment-runner.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ArchiveTest(unittest.TestCase):
    def extract(self, member):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            archive = base / "source.tar"
            with tarfile.open(archive, "w") as tar:
                tar.addfile(member, io.BytesIO(b"test") if member.isfile() else None)
            output = base / "out"
            output.mkdir()
            runner.extract(archive, output)
            return (output / member.name).read_bytes()

    def test_regular_file(self):
        member = tarfile.TarInfo("folder/file")
        member.size = 4
        self.assertEqual(self.extract(member), b"test")

    def test_no_escape_paths(self):
        for name in ["../outside", "/absolute", "folder/../../outside"]:
            with self.subTest(name=name), self.assertRaises(RuntimeError):
                member = tarfile.TarInfo(name)
                member.size = 4
                self.extract(member)

    def test_no_links_or_devices(self):
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.FIFOTYPE]:
            with self.subTest(kind=kind), self.assertRaises(RuntimeError):
                member = tarfile.TarInfo("redirect")
                member.type = kind
                member.linkname = "/outside"
                self.extract(member)


if __name__ == "__main__":
    unittest.main()
